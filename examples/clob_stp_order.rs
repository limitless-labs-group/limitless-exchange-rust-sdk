mod support;

use limitless_exchange_rust_sdk::{
    CreateOrderParams, GtcOrderArgs, OrderArgs, OrderType, Side, StpPolicy,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sdk = support::api_key_client()?;
    let private_key = support::require_env("PRIVATE_KEY");
    let market_slug = support::optional_env_with_fallback("MARKET_SLUG", "will-btc-hit-100k");

    let market = sdk.markets.get_market(&market_slug).await?;
    let orderbook = sdk.markets.get_order_book(&market_slug).await?;
    let tokens = support::required_market_tokens(&market)?;

    println!("Market: {}", market.title);
    println!("Orderbook midpoint: {:.3}\n", orderbook.adjusted_midpoint);

    let order_client = sdk.new_order_client(&private_key, None)?;
    let response = order_client
        .create_order(CreateOrderParams {
            order_type: OrderType::Gtc,
            market_slug: market_slug.clone(),
            args: OrderArgs::from(GtcOrderArgs {
                token_id: tokens.yes,
                side: Side::Buy,
                price: 0.500,
                size: 10.0,
                expiration: None,
                nonce: None,
                taker: None,
                post_only: false,
            }),
            // Self-trade prevention: if this taker crosses your own resting
            // maker orders, cancel the resting maker side and let the taker
            // continue. Omit `stp_policy` to use the engine default
            // (`cancel_maker`). `cancel_taker` and `cancel_both` are also
            // available. `stpPolicy` is sent as a top-level request field and is
            // never part of the signed order.
            stp_policy: Some(StpPolicy::CancelMaker),
        })
        .await?;

    println!("Order created: {}", response.order.id);
    println!("  Price: {:?}", response.order.price);
    println!("  Maker Amount: {}", response.order.maker_amount);
    println!("  Taker Amount: {}", response.order.taker_amount);

    // The execution object is always present on a live response. It reports the
    // settlement status, fees, and any self-trade-prevention signals.
    let execution = &response.execution;
    println!("\nExecution:");
    println!("  Matched: {}", execution.matched);
    println!("  Settlement status: {}", execution.settlement_status);
    println!("  Effective fee bps: {}", execution.effective_fee_bps);
    println!("  USD net: {}", execution.totals_raw.usd_net);
    if let Some(reason) = &execution.reason {
        // A self-trade-prevented taker reject reports e.g. STP_TAKER_REJECTED.
        println!("  Reason: {reason}");
    }
    if let Some(canceled) = &execution.stp_maker_cancels {
        // Maker order ids canceled by self-trade prevention.
        println!("  STP maker cancels: {canceled:?}");
    }

    let message = order_client.cancel(&response.order.id).await?;
    println!("\nOrder cancelled: {message}");

    Ok(())
}
