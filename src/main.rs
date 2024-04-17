use keom_clob::{
    build_consume_order_tx_req, create_and_submit_limit_order, create_client, execute,
    get_accounts, mint,
};
use tracing::{info, span, Level};

use miden_objects::assets::FungibleAsset;
use tracing::event;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(fmt::layer().with_timer(fmt::time::Uptime::default()))
        .with(EnvFilter::from_default_env())
        .init();
    info!("Creating client...");
    let mut client = create_client();

    let (maker, taker, eth, dai) = get_accounts();
    let eth_asset = FungibleAsset::new(eth.id(), 1).unwrap();
    let dai_asset = FungibleAsset::new(dai.id(), 5).unwrap();
    info!("Created accounts");
    info!(
        maker = maker.id().to_hex(),
        taker = taker.id().to_hex(),
        eth = eth.id().to_hex(),
        dai = dai.id().to_hex(),
    );

    info!("Minting assets to maker");
    info!(mintTo = maker.id().to_hex(), asset = "eth", amount = eth_asset.amount());
    mint(&maker, eth_asset).await;

    info!("Minting assets to taker");
    info!(mintTo = taker.id().to_hex(), asset = "dai", amount = dai_asset.amount());
    mint(&taker, dai_asset).await;

    // sync
    client.sync_state().await.unwrap();

    info!("Creating limit order");
    // Submit a transaction to create the note
    let (_tx_id, created_notes) =
        create_and_submit_limit_order(&client, &maker, eth_asset.into(), dai_asset.into()).await;
    info!(tx = _tx_id.to_hex(), output_note_len = created_notes.len());

    let created_note = created_notes.first().unwrap();

    // consume it
    client.sync_state().await.unwrap();
    info!(note = created_note.id().to_hex(), "Taker consuming note");

    // TODO: consume note from the taker account
    //       this transaction should fail because the taker does not have the required assets
    let tx_req =
        build_consume_order_tx_req(created_note.id(), maker.clone(), 1, 5, dai_asset.into());
    execute(tx_req).await;

    // display
    let (maker, _) = client.get_account(maker.id()).unwrap();
    let (taker, _) = client.get_account(taker.id()).unwrap();
    info!("Account balances");
    info!(
        acc = "maker",
        eth = maker.vault().get_balance(eth.id()).unwrap(),
        dai = maker.vault().get_balance(dai.id()).unwrap()
    );
    client.sync_state().await.unwrap();
}
