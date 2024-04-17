pub mod helpers;

//use super::*;
use keom_clob::limit_swap::{build_partial_recipient, create_limit_swap_note};
use miden_lib::notes::utils::{build_note_script, build_p2id_recipient};
use miden_mock::constants::{
    ACCOUNT_ID_NON_FUNGIBLE_FAUCET_ON_CHAIN, ACCOUNT_ID_REGULAR_ACCOUNT_UPDATABLE_CODE_ON_CHAIN,
    ACCOUNT_ID_SENDER, DEFAULT_AUTH_SCRIPT,
};
use miden_objects::accounts::{Account, AccountId, ACCOUNT_ID_FUNGIBLE_FAUCET_ON_CHAIN};
use miden_objects::assembly::ProgramAst;
use miden_objects::assets::{
    Asset, AssetVault, FungibleAsset, NonFungibleAsset, NonFungibleAssetDetails,
};
use miden_objects::crypto::rand::RpoRandomCoin;
use miden_objects::notes::{Note, NoteAssets, NoteInputs, NoteMetadata, NoteRecipient};
use miden_objects::transaction::{OutputNote, TransactionArgs};
use miden_objects::vm::AdviceMap;
use miden_objects::{Felt, Hasher, Word, ZERO};
use miden_tx::{ProvingOptions, TransactionExecutor, TransactionProver};
use std::collections::BTreeMap;

use crate::helpers::{
    get_account_with_default_account_code, get_new_key_pair_with_advice_map,
    prove_and_verify_transaction, MockDataStore,
};

#[test]
fn prove_limit_swap_script() {
    // Maker's initial offered/desired of limit swap order
    let amount_offered: u64 = 100;
    let amount_desired: u64 = 50;

    // Taker's desried amount to consume/satisfy
    let amount_to_consume: u64 = 50;
    let amount_to_send: u64 = 25;

    // Maker's new offered/desired update to limit swap order
    let new_amount_offered: u64 = 10;
    let new_amount_desired: u64 = 7;

    // Dummy faucet IDs for assets A and B
    let asset_a_id: u64 = 10000118204333965312;
    let asset_b_id: u64 = 10000344073709551615;

    ///////////////////
    // Create Assets //
    ///////////////////

    // Asset A
    let faucet_id_a = AccountId::try_from(asset_a_id).unwrap();
    let fungible_asset_a: Word = FungibleAsset::new(faucet_id_a, amount_offered).unwrap().into();
    println!("Offered Asset ID {}", fungible_asset_a[3]);
    let fungible_asset_a: Asset = FungibleAsset::new(faucet_id_a, amount_offered).unwrap().into();

    // Asset B
    let faucet_id_b = AccountId::try_from(asset_b_id).unwrap();
    let fungible_asset_b: Word = FungibleAsset::new(faucet_id_b, amount_desired).unwrap().into();
    println!("Desired Asset ID {}", fungible_asset_b[3]);
    let fungible_asset_b: Asset = FungibleAsset::new(faucet_id_b, amount_desired).unwrap().into();

    /////////////////////
    // Create Accounts //
    /////////////////////

    // Maker Account
    // Initialized without funds in account

    let maker_account_id = AccountId::try_from(ACCOUNT_ID_SENDER).unwrap();
    let (maker_pub_key, maker_sk_felt) = get_new_key_pair_with_advice_map();
    let maker_account =
        get_account_with_default_account_code(maker_account_id, maker_pub_key, None);

    println!(">>>>>> Built agent: Maker");

    // Initial limit swap note issued by maker
    // Maker offers 100 token_a for 50 token_B

    let random_val = RpoRandomCoin::new([Felt::new(1), Felt::new(2), Felt::new(3), Felt::new(4)]);
    let (limit_swap_note, payback_serial_num, note_serial_num, note_script, maker, tag) =
        create_limit_swap_note(maker_account_id, fungible_asset_a, fungible_asset_b, random_val)
            .unwrap();

    assert_eq!(maker_account_id, maker);
    println!(">>>>>> Built Maker's limit order note");

    // Taker Account
    // Initialized with the entirety of asset b

    let taker_account_id =
        AccountId::try_from(ACCOUNT_ID_REGULAR_ACCOUNT_UPDATABLE_CODE_ON_CHAIN).unwrap();
    let (taker_pub_key, taker_sk_felt) = get_new_key_pair_with_advice_map();
    let taker_account = get_account_with_default_account_code(
        taker_account_id,
        taker_pub_key,
        Some(fungible_asset_b),
    );

    taker_account.vault().assets().for_each(|asset| {
        println!("Taker asset: {:?}", asset);
    });

    println!(">>>>>> Built Agent: Taker");

    ////////////////////////////////////////
    //         >>> TAKER TESTS <<<        //
    //     CONSTRUCT AND EXECUTE TX       //
    //.     Test for success              //
    ////////////////////////////////////////

    // Build dummy blockchain state as seen by Taker

    // TODO: read from blockchain
    let taker_data_store = MockDataStore::with_existing(
        Some(taker_account.clone()),
        Some(vec![limit_swap_note.clone()]),
    );

    let block_ref = taker_data_store.block_header.block_num();
    let note_ids = vec![limit_swap_note.id()];

    println!(">>>>>> Initialized Taker's dummy blockchain");

    // Build Taker's TX executor

    let mut taker_executor = TransactionExecutor::new(taker_data_store.clone());
    taker_executor.load_account(taker_account_id).unwrap();

    // Construct template transaction script for Taker

    let tx_script_code = ProgramAst::parse(DEFAULT_AUTH_SCRIPT).unwrap();
    let tx_script_target = taker_executor
        .compile_tx_script(tx_script_code.clone(), vec![(taker_pub_key, taker_sk_felt)], vec![])
        .unwrap();

    // Build input arguments for Note consumption
    // Taker will consume amount_to_consume and satisfy amount_to_send
    // of Maker's initial request

    let mut note_args_map = BTreeMap::new();
    let takers_args: Word = [
        ZERO,
        ZERO,
        Felt::new(amount_to_send),
        Felt::new(amount_to_consume),
    ];
    note_args_map.insert(limit_swap_note.id(), takers_args);

    let tx_args_taker =
        TransactionArgs::new(Some(tx_script_target), Some(note_args_map), AdviceMap::new());

    // Execute Taker transaction

    let transaction_result = taker_executor
        .execute_transaction(taker_account_id, block_ref, &note_ids, tx_args_taker)
        .unwrap();

    println!(">>>>>> Executed Taker's transaction");

    println!("Taker's account delta {:#?}", transaction_result.account_delta());

    // Verify that two Notes have been created (a p2id and a limit_swap clone)

    assert_eq!(transaction_result.output_notes().num_notes(), 2);

    println!(">>>>>> Verified that Taker's transaction created two notes");

    // Check if the created `P2ID Note` is what we expect
    // First by constructing what the Note should look like

    let p2id_recipient = build_p2id_recipient(maker_account_id, payback_serial_num).unwrap();
    let p2id_metadata = NoteMetadata::new(
        taker_account_id,
        miden_objects::notes::NoteType::OffChain,
        maker_account_id.into(),
        tag, //TODO: use random aux
    );
    let sent_fungible_asset_b: Asset =
        FungibleAsset::new(faucet_id_b, amount_to_send).unwrap().into();
    let consumed_fungible_asset_a: Asset =
        FungibleAsset::new(faucet_id_a, amount_to_consume).unwrap().into();

    let p2id_assets = NoteAssets::new(vec![sent_fungible_asset_b]).unwrap();

    //TODO: fix args
    let p2id_expected_note = Note::new(
        p2id_assets,
        p2id_metadata.unwrap(),
        NoteRecipient::new(note_serial_num, note_script, NoteInputs::new(vec![]).unwrap()),
    );

    // Extracting the OutputNote format as returned by the transaction executor

    let p2id_created_note = transaction_result.output_notes().get_note(0);

    // And asserting that they are the same
    assert_eq!(p2id_created_note.id(), p2id_expected_note.id());

    println!(">>>>>> Verified that the output P2ID note was built as expected");

    // Check if the created `LIMIT_SWAP Clone Note` is what we expect
    // First by constructing what the Note should look like

    let bytes = include_bytes!(concat!(env!("OUT_DIR"), "/assets/note_scripts/limit_swap.masb"));
    let note_script = build_note_script(bytes);

    let partial_recipient =
        build_partial_recipient(note_script.unwrap().clone(), note_serial_num).unwrap();
    let p2id_recipient = build_p2id_recipient(maker_account_id, payback_serial_num).unwrap();

    let fungible_asset_a_leftover: Asset =
        FungibleAsset::new(faucet_id_a, amount_offered - amount_to_consume).unwrap().into();
    let fungible_asset_b_missing: Word =
        FungibleAsset::new(faucet_id_b, amount_desired - amount_to_send).unwrap().into();

    let new_inputs = [
        p2id_recipient[0],
        p2id_recipient[1],
        p2id_recipient[2],
        p2id_recipient[3],
        fungible_asset_b_missing[0],
        fungible_asset_b_missing[1],
        fungible_asset_b_missing[2],
        fungible_asset_b_missing[3],
        maker_account_id.into(),
        ZERO,
        ZERO,
        ZERO,
        partial_recipient[0],
        partial_recipient[1],
        partial_recipient[2],
        partial_recipient[3],
    ];

    println!("Taker Inputs {:#?}", new_inputs);

    let limit_clone_inputs_hash = Hasher::hash_elements(&new_inputs);
    println!(">>> Expected Inputs Hash {:#?}", limit_clone_inputs_hash);

    let limit_clone_recipient = Hasher::merge(&[partial_recipient, limit_clone_inputs_hash]);
    println!(">>> Expected Recipient {:#?}", limit_clone_recipient);

    let limit_clone_metadata = NoteMetadata::new(
        taker_account_id,
        miden_objects::notes::NoteType::OffChain,
        maker_account_id.into(),
        tag,
    );
    let limit_clone_assets = NoteAssets::new(vec![fungible_asset_a_leftover]).unwrap();
    let limit_clone_expected_note =
        OutputNote::new(limit_clone_recipient, limit_clone_assets, limit_clone_metadata);

    // Extracting the OutputNote format as returned by the transaction executor

    let limit_clone_created_note = transaction_result.output_notes().get_note(1);

    // And asserting that they are the same

    assert_eq!(limit_clone_created_note.clone(), limit_clone_expected_note);

    ////////////////////////////////////////
    //         >>> MAKER TESTS <<<        //
    //     CONSTRUCT AND EXECUTE TX       //
    //          Test for success          //
    ////////////////////////////////////////

    // Rebuild limit clone note from scratch as returned by Bob above

    // We copy the logic from create_limit_swap_note()
    // by contructing the note script

    let bytes = include_bytes!(concat!(env!("OUT_DIR"), "/assets/note_scripts/limit_swap.masb"));
    let limit_swap_note_script = build_note_script(bytes);

    println!("Asset A leftover {:#?}", fungible_asset_a_leftover);

    // and the full Note while reusing note_serial_num, maker, tag from the original
    // create_limit_swap_note() call

    let limit_clone_note = Note::new(
        limit_swap_note_script.unwrap(),
        &new_inputs,
        &[fungible_asset_a_leftover],
        note_serial_num,
        maker,
        tag,
    )
    .unwrap();

    // We extract the recipient from this note
    let limit_clone_note_recipient = limit_clone_note.recipient();

    // and the one created by Bob's executor
    let created_limit_clone_note_recipient = limit_clone_created_note.clone();

    // and assert that they're identical.
    assert_eq!(limit_clone_note_recipient, created_limit_clone_note_recipient.recipient().clone());
    println!(">>>>>> New dummy clone note matches output from Taker tests");

    // Build dummy blockchain state as seen by Maker

    let maker_data_store = MockDataStore::with_existing(
        Some(maker_account.clone()),
        Some(vec![limit_clone_note.clone()]),
    );

    let block_ref = maker_data_store.block_header.block_num();
    let limit_clone_ids =
        maker_data_store.notes.iter().map(|note| limit_clone_note.id()).collect::<Vec<_>>();

    println!(">>>>>> Initialized Maker's's dummy blockchain");

    // Build Taker's TX executor

    let mut maker_executor = TransactionExecutor::new(maker_data_store.clone());
    maker_executor.load_account(maker_account_id).unwrap();

    // Build transaction script

    let tx_script_target = maker_executor
        .compile_tx_script(tx_script_code.clone(), vec![(maker_pub_key, maker_sk_felt)], vec![])
        .unwrap();

    println!(">>>>>> Initialized Maker's Executor");

    // Load inputs such that Maker updates new_amount_desired
    // and new_amount_offered

    let mut note_args_map = BTreeMap::new();
    let maker_args: Word = [
        ZERO,
        ZERO,
        Felt::new(new_amount_desired),
        Felt::new(new_amount_offered),
    ];
    note_args_map.insert(limit_clone_created_note.id(), maker_args);

    let tx_args_maker = TransactionArgs::new(Some(tx_script_target), Some(note_args_map));

    // Execute the transaction where Maker updates the limit-swap clone

    let transaction_result = maker_executor
        .execute_transaction(maker_account_id, block_ref, &limit_clone_ids, Some(tx_args_maker))
        .unwrap();

    // measure time
    let start = std::time::Instant::now();
    println!(">>>>>> Proving Taker's tx");
    let transaction_prover = TransactionProver::new(ProvingOptions::default());
    let proven_transaction = transaction_prover.prove_transaction(transaction_result.clone());
    let elapsed = start.elapsed();
    println!(">>>>>> Proving Taker's tx took {:?}", elapsed);
    println!(">>>>>> Executed Makers transaction");
    println!("Maker's account delta {:#?}", transaction_result.account_delta());

    // Check that only up-to-date note exists

    assert_eq!(transaction_result.output_notes().num_notes(), 1);
    println!(">>>>>> Asserted leftover notes");

    let fungible_asset_a_leftover: Asset =
        FungibleAsset::new(faucet_id_a, new_amount_offered).unwrap().into();
    let fungible_asset_b_missing: Word =
        FungibleAsset::new(faucet_id_b, new_amount_desired).unwrap().into();

    let new_inputs = [
        p2id_recipient[0],
        p2id_recipient[1],
        p2id_recipient[2],
        p2id_recipient[3],
        fungible_asset_b_missing[0],
        fungible_asset_b_missing[1],
        fungible_asset_b_missing[2],
        fungible_asset_b_missing[3],
        maker_account_id.into(),
        ZERO,
        ZERO,
        ZERO,
        partial_recipient[0],
        partial_recipient[1],
        partial_recipient[2],
        partial_recipient[3],
    ];

    println!("Maker Inputs {:#?}", new_inputs);

    let limit_clone_inputs_hash = Hasher::hash_elements(&new_inputs);
    println!(">>> Expected Inputs Hash {:#?}", limit_clone_inputs_hash);

    let limit_clone_recipient = Hasher::merge(&[partial_recipient, limit_clone_inputs_hash]);
    println!(">>> Expected Recipient {:#?}", limit_clone_recipient);

    let limit_clone_metadata = NoteMetadata::new(maker_account_id, maker_account_id.into());
    let limit_clone_assets = NoteAssets::new(&[fungible_asset_a_leftover]).unwrap();
    let limit_clone_expected_note =
        OutputNote::new(limit_clone_recipient, limit_clone_assets, limit_clone_metadata);

    let limit_clone_created_note = transaction_result.output_notes().get_note(0);

    println!(">>> Created Recipient {:#?}", limit_clone_created_note.recipient());

    assert_eq!(limit_clone_created_note.clone(), limit_clone_expected_note);
}
