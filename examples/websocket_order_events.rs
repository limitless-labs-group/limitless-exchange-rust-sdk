mod support;

use limitless_exchange_rust_sdk::{OrderEvent, SubscriptionChannel, SubscriptionOptions};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sdk = support::hmac_or_api_key_client()?;
    let ws = sdk.new_websocket_client(None);
    let market_slug = support::optional_env_with_fallback("MARKET_SLUG", "will-btc-hit-100k");

    ws.on_order_event_typed(|event| match event {
        OrderEvent::Matched(matched) => {
            println!(
                "\nMATCHED fill: price={} token={:?} side={:?} estimate={:?}",
                matched.price, matched.token, matched.side, matched.is_estimate
            );
            if let Some(effective_fee_bps) = matched.effective_fee_bps {
                println!("  Effective fee (bps): {}", effective_fee_bps);
            }
        }
        OrderEvent::Execution(execution) => {
            println!(
                "\nEXECUTION terminal: status={} price={} remaining={} token={} side={}",
                execution.status,
                execution.price.float64(),
                execution.remaining_size.float64(),
                execution.token,
                execution.side
            );
            println!("  Order: {}", execution.order_id);
        }
        OrderEvent::Unknown => {}
    });

    ws.connect().await?;
    ws.subscribe(
        SubscriptionChannel::SubscribeOrderEvents,
        SubscriptionOptions::default(),
    )
    .await?;

    println!(
        "Subscribed to order events (market filter: {}). Press Ctrl+C to exit.",
        market_slug
    );
    tokio::signal::ctrl_c().await?;
    println!("\nShutting down...");
    ws.disconnect().await?;
    Ok(())
}
