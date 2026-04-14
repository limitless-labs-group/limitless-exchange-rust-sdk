mod support;

use limitless_exchange_rust_sdk::{SubscriptionChannel, SubscriptionOptions};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sdk = support::hmac_or_api_key_client()?;
    let ws = sdk.new_websocket_client(None);
    let market_slug = support::optional_env_with_fallback("MARKET_SLUG", "will-btc-hit-100k");

    ws.on("positions", |data| {
        println!("\nPosition update: {}", data);
    });

    ws.on_transaction(|tx| {
        println!("\nTransaction: status={} source={}", tx.status, tx.source);
        if let Some(market_slug) = tx.market_slug {
            println!("  Market: {}", market_slug);
        }
        if let Some(tx_hash) = tx.tx_hash {
            println!("  TxHash: {}", tx_hash);
        }
    });

    ws.connect().await?;
    ws.subscribe(
        SubscriptionChannel::SubscribePositions,
        SubscriptionOptions {
            market_slugs: vec![market_slug.clone()],
            ..Default::default()
        },
    )
    .await?;
    ws.subscribe(
        SubscriptionChannel::SubscribeTransactions,
        SubscriptionOptions::default(),
    )
    .await?;

    println!(
        "Subscribed to positions for {}. Press Ctrl+C to exit.",
        market_slug
    );
    tokio::signal::ctrl_c().await?;
    println!("\nShutting down...");
    ws.disconnect().await?;
    Ok(())
}
