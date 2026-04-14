mod support;

use std::io;

use limitless_exchange_rust_sdk::{CreateOrderParams, GtcOrderArgs, OrderArgs, OrderType, Side};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sdk = support::api_key_client()?;
    let private_key = support::require_env("PRIVATE_KEY");
    let market_slug =
        support::optional_env_with_fallback("MARKET_SLUG", "us-presidential-election-2024");

    let market = sdk.markets.get_market(&market_slug).await?;
    println!("Group Market: {}", market.title);
    println!("  Sub-markets: {}\n", market.markets.len());

    let sub_market = market
        .markets
        .first()
        .cloned()
        .ok_or_else(|| io::Error::other("No sub-markets found"))?;
    let tokens = sub_market
        .tokens
        .clone()
        .ok_or_else(|| io::Error::other("Sub-market has no tokens"))?;

    println!("Sub-market: {}", sub_market.title);

    let order_client = sdk.new_order_client(&private_key, None)?;
    let response = order_client
        .create_order(CreateOrderParams {
            order_type: OrderType::Gtc,
            market_slug: sub_market.slug,
            args: OrderArgs::from(GtcOrderArgs {
                token_id: tokens.yes,
                side: Side::Buy,
                price: 0.600,
                size: 5.0,
                expiration: None,
                nonce: None,
                taker: None,
                post_only: false,
            }),
        })
        .await?;

    println!("Order created: {}", response.order.id);

    Ok(())
}
