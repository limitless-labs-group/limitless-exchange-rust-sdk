mod support;

use limitless_exchange_rust_sdk::{SubscriptionChannel, SubscriptionOptions};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sdk = support::public_client()?;
    let ws = sdk.new_websocket_client(None);
    let market_slug = support::optional_env_with_fallback("MARKET_SLUG", "will-btc-hit-100k");

    ws.on_orderbook_update(|update| {
        println!("\nOrderbook update for {}:", update.market_slug);
        println!(
            "  Bids: {}, Asks: {}",
            update.orderbook.bids.len(),
            update.orderbook.asks.len()
        );
        println!("  Midpoint: {:.3}", update.orderbook.adjusted_midpoint);

        if let Some(best_bid) = update.orderbook.bids.first() {
            println!(
                "  Best bid: {:.3} (size: {:.2})",
                best_bid.price, best_bid.size
            );
        }
        if let Some(best_ask) = update.orderbook.asks.first() {
            println!(
                "  Best ask: {:.3} (size: {:.2})",
                best_ask.price, best_ask.size
            );
        }
    });

    ws.connect().await?;
    ws.subscribe(
        SubscriptionChannel::Orderbook,
        SubscriptionOptions {
            market_slugs: vec![market_slug.clone()],
            ..Default::default()
        },
    )
    .await?;

    println!(
        "Subscribed to orderbook for {}. Press Ctrl+C to exit.",
        market_slug
    );
    tokio::signal::ctrl_c().await?;
    println!("\nShutting down...");
    ws.disconnect().await?;
    Ok(())
}
