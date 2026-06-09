mod support;

use limitless_exchange_rust_sdk::{CreateOrderParams, FakOrderArgs, OrderArgs, OrderType, Side};

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
            order_type: OrderType::Fak,
            market_slug,
            args: OrderArgs::from(FakOrderArgs {
                token_id: tokens.yes,
                side: Side::Buy,
                price: 0.450,
                size: 10.0,
                expiration: None,
                nonce: None,
                taker: None,
            }),
            stp_policy: None,
        })
        .await?;

    println!("FAK order created: {}", response.order.id);
    println!("  Price: {:?}", response.order.price);
    println!("  Maker Amount: {}", response.order.maker_amount);
    println!("  Taker Amount: {}", response.order.taker_amount);

    if !response.maker_matches.is_empty() {
        println!(
            "  Matched immediately with {} fill(s)",
            response.maker_matches.len()
        );
    } else {
        println!("  No immediate match. Unfilled remainder was cancelled by FAK semantics.");
    }

    Ok(())
}
