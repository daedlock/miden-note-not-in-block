use tracing::{debug, info, instrument};
use vm_processor::{crypto::RpoRandomCoin, Digest, Felt, Word, ZERO};

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::vec;

use miden_client::client::accounts::AccountTemplate;
use miden_client::client::rpc::TonicRpcClient;
use miden_client::client::transactions::transaction_request::{
    NoteArgs, TransactionRequest, TransactionTemplate, AUTH_SEND_ASSET_SCRIPT,
};
use miden_client::client::{self, get_random_coin, Client};
use miden_client::config::{ClientConfig, RpcConfig};
use miden_client::store::sqlite_store::SqliteStore;
use miden_client::store::TransactionFilter;

use miden_lib::transaction::TransactionKernel;
use miden_mock::utils::prepare_word;
use miden_objects::accounts::{Account, AccountId};
use miden_objects::assembly::ProgramAst;
use miden_objects::assets::{Asset, FungibleAsset, TokenSymbol};
use miden_objects::crypto::rand::FeltRng;
use miden_objects::notes::{
    Note, NoteAssets, NoteId, NoteInputs, NoteMetadata, NoteRecipient, NoteScript, NoteTag,
};
use miden_objects::transaction::TransactionId;

use miden_objects::{Hasher, NoteError};
pub type MidenClient = Client<TonicRpcClient, RpoRandomCoin, SqliteStore>;

/// Creates the partial_RECIPIENT for generating note clones
#[instrument]
pub fn build_partial_recipient(
    note_script: NoteScript,
    serial_num: Word,
) -> Result<Digest, NoteError> {
    let script_hash = note_script.hash();

    let serial_num_hash = Hasher::merge(&[serial_num.into(), Digest::default()]);

    Ok(Hasher::merge(&[serial_num_hash, script_hash]))
}

/// Creates a limit swap note for the maker account
/// offering a certain amount of an asset in exchange for another asset
/// The note code is in the masm/limit_swap.masm
pub fn create_limit_swap_note<R: FeltRng>(
    maker: AccountId,
    offered_asset: Asset,
    requested_asset: Asset,
    mut rng: R,
) -> Result<Note, NoteError> {
    let assembler = TransactionKernel::assembler();

    let note_script = include_str!("./masm/limit_swap.masm");
    let note_script = ProgramAst::parse(note_script).unwrap();
    let (note_script, _) = NoteScript::new(note_script, &assembler).unwrap();

    let payback_serial_num = rng.draw_word();
    let note_serial_num = rng.draw_word();

    let p2id_recipient = miden_lib::notes::utils::build_p2id_recipient(maker, payback_serial_num)?;
    let partial_recipient = build_partial_recipient(note_script.clone(), note_serial_num)?;
    let requested_asset_word: Word = requested_asset.into();

    let inputs = [
        p2id_recipient[0],
        p2id_recipient[1],
        p2id_recipient[2],
        p2id_recipient[3],
        requested_asset_word[0],
        requested_asset_word[1],
        requested_asset_word[2],
        requested_asset_word[3],
        maker.into(),
        ZERO,
        ZERO,
        ZERO,
        partial_recipient[0],
        partial_recipient[1],
        partial_recipient[2],
        partial_recipient[3],
    ];

    let note_assets = NoteAssets::new(vec![offered_asset]).unwrap();
    let note_recipient = NoteRecipient::new(
        note_serial_num,
        note_script.clone(),
        NoteInputs::new(inputs.to_vec()).unwrap(),
    );
    let note_metadata = NoteMetadata::new(
        maker,
        miden_objects::notes::NoteType::Public,
        NoteTag::from_account_id(maker, miden_objects::notes::NoteExecutionMode::Local).unwrap(), //TODO: change tag
        rng.draw_element(),
    )
    .unwrap();

    let note = Note::new(note_assets, note_metadata, note_recipient);

    Ok(note)
}

/// Execute a transaction and wait for it to be committed by the node
#[instrument(skip_all)]
pub async fn execute(tx_request: TransactionRequest) -> (TransactionId, Vec<Note>) {
    let mut client = create_client();
    client.sync_state().await.unwrap();
    let transaction_execution_result = client.new_transaction(tx_request).unwrap();
    let transaction_id = transaction_execution_result.executed_transaction().id();
    let created_notes = transaction_execution_result.created_notes().to_vec();

    info!(transaction_id = transaction_id.to_hex(), "Sending transaction to node");
    client.submit_transaction(transaction_execution_result).await.unwrap();

    loop {
        debug!(cur_block = client.get_sync_height().unwrap(), "Syncing state...");
        client.sync_state().await.unwrap();

        // Check if executed transaction got committed by the node
        let uncommited_transactions =
            client.get_transactions(TransactionFilter::Uncomitted).unwrap();
        let is_tx_committed =
            !uncommited_transactions.iter().any(|uncommited_tx| uncommited_tx.id == transaction_id);

        if is_tx_committed {
            break;
        }

        std::thread::sleep(std::time::Duration::new(3, 0));
    }
    (transaction_id, created_notes)
}

/// Create a miden client
pub fn create_client() -> MidenClient {
    let mut file = PathBuf::from("./db");
    file.push(format!("{}.sqlite3", "miden-db"));
    let client_config = ClientConfig {
        store: file.into_os_string().into_string().unwrap().try_into().unwrap(),
        rpc: RpcConfig::default(),
    };

    let rpc_endpoint = client_config.rpc.endpoint.to_string();
    let store = SqliteStore::new((&client_config).into()).unwrap();
    let executor_store = SqliteStore::new((&client_config).into()).unwrap();
    let rng = get_random_coin();

    Client::new(TonicRpcClient::new(&rpc_endpoint), rng, store, executor_store).unwrap()
}

/// Create accounts for the maker, taker, eth, dai
pub fn get_accounts() -> (Account, Account, Account, Account) {
    let mut client = create_client();
    let (maker, _) = client
        .new_account(AccountTemplate::BasicWallet {
            mutable_code: false,
            storage_mode: client::accounts::AccountStorageMode::Local,
        })
        .unwrap();
    let (taker, _) = client
        .new_account(AccountTemplate::BasicWallet {
            mutable_code: false,
            storage_mode: client::accounts::AccountStorageMode::Local,
        })
        .unwrap();

    let (eth_fauc, _) = client
        .new_account(AccountTemplate::FungibleFaucet {
            token_symbol: TokenSymbol::new("ETH").unwrap(),
            decimals: 8,
            max_supply: 100_000_000_000,
            storage_mode: client::accounts::AccountStorageMode::Local,
        })
        .unwrap();
    let (dai_fauc, _) = client
        .new_account(AccountTemplate::FungibleFaucet {
            token_symbol: TokenSymbol::new("DAI").unwrap(),
            decimals: 8,
            max_supply: 100_000_000_000,
            storage_mode: client::accounts::AccountStorageMode::Local,
        })
        .unwrap();

    (maker, taker, eth_fauc, dai_fauc)
}

/// Builds a transaction request to consume a
/// limit order note
#[instrument(skip_all, fields(note_id, taker=taker.id().to_hex(), out_asset_amount, in_asset_amount))]
pub fn build_consume_order_tx_req(
    note_id: NoteId,
    taker: Account,
    out_asset_amount: u64,
    in_asset_amount: u64,
    in_asset: Asset,
) -> TransactionRequest {
    let client = create_client();
    let note = client.get_input_note(note_id).unwrap();
    let mut note_tree: BTreeMap<NoteId, Option<NoteArgs>> = BTreeMap::new();
    let taker_args: Word = [
        ZERO,
        ZERO,
        Felt::new(in_asset_amount),
        Felt::new(out_asset_amount),
    ];
    note_tree.insert(note_id, Some(taker_args));

    // build recipient
    let recipient =
        note.recipient().iter().map(|x| x.as_int().to_string()).collect::<Vec<_>>().join(".");
    let note_tag = note.metadata().unwrap().tag();

    let tx_ast = ProgramAst::parse(
        &AUTH_SEND_ASSET_SCRIPT
            .replace("{recipient}", &recipient)
            .replace(
                "{note_type}",
                &Felt::new(note.metadata().unwrap().note_type() as u64).to_string(),
            )
            .replace("{tag}", &Felt::new(note_tag.into()).to_string())
            .replace("{asset}", &prepare_word(&in_asset.into()).to_string())
            .to_string(),
    )
    .unwrap();
    let auth = client.get_account_auth(taker.id()).unwrap();
    let script_inputs = vec![auth.into_advice_inputs()];
    let tx_script = client.compile_tx_script(tx_ast, script_inputs, vec![]).unwrap();

    TransactionRequest::new(taker.id(), note_tree, vec![], Some(tx_script))
}

/// mints an asset to an account in a transaction.
/// The output note is consumed in another transaction
/// and is submitted to the node.
#[instrument(skip_all, fields(acc = account.id().to_hex()))]
pub async fn mint(account: &Account, faucet: FungibleAsset) {
    let mut client = create_client();
    let tx_template = TransactionTemplate::MintFungibleAsset(
        faucet,
        account.id(),
        miden_objects::notes::NoteType::OffChain,
    );

    let tx_request = client.build_transaction_request(tx_template).unwrap();
    let (_tx_id, consumables) = execute(tx_request).await;
    for note in consumables {
        info!(id = note.id().to_hex(), "Consuming note");
        // consume
        let tx_template = TransactionTemplate::ConsumeNotes(account.id(), vec![note.id()]);
        let tx_req = client.build_transaction_request(tx_template).unwrap();
        let (tx_id, _created_notes) = execute(tx_req).await;
        info!(id = note.id().to_hex(), "Consumed note");
    }
}

/// Build miden transaction to create a limit order and submit it to the network.
/// The transaction creates an output note that can be consumed by a taker to
/// execute the swap fully or partially.
#[instrument(skip_all,fields(maker_id = maker.id().to_hex(), from_asset = ?from_asset, to_asset = ?to_asset))]
pub async fn create_and_submit_limit_order(
    client: &MidenClient,
    maker: &Account,
    from_asset: Asset,
    to_asset: Asset,
) -> (TransactionId, Vec<Note>) {
    let felt_rng = get_random_coin();
    let limit_swap_note =
        create_limit_swap_note(maker.id(), from_asset, to_asset, felt_rng).unwrap();
    let note_tag = limit_swap_note.metadata().tag().inner();

    debug!(name: "create_and_submit_limit_order",  tag=note_tag);

    // build recipient
    let recipient = limit_swap_note
        .recipient_digest()
        .iter()
        .map(|x| x.as_int().to_string())
        .collect::<Vec<_>>()
        .join(".");
    debug!(name: "create_and_submit_limit_order",  recipient=recipient);

    let tx_ast = ProgramAst::parse(
        &AUTH_SEND_ASSET_SCRIPT
            .replace("{recipient}", &recipient)
            .replace(
                "{note_type}",
                &Felt::new(limit_swap_note.metadata().note_type() as u64).to_string(),
            )
            .replace("{tag}", &Felt::new(note_tag.into()).to_string())
            .replace("{asset}", &prepare_word(&from_asset.into()).to_string())
            .to_string(),
    )
    .unwrap();

    let auth = client.get_account_auth(maker.id()).unwrap();
    let script_inputs = vec![auth.into_advice_inputs()];

    let tx_script = client.compile_tx_script(tx_ast, script_inputs, vec![]).unwrap();

    // build tx req
    let tx_req = TransactionRequest::new(
        maker.id(),
        BTreeMap::new(),
        vec![limit_swap_note],
        Some(tx_script),
    );

    execute(tx_req).await
}
