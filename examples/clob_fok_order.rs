mod support;

use limitless_exchange_rust_sdk::{CreateOrderParams, FokOrderArgs, OrderArgs, OrderType, Side};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sdk = support::api_key_client()?;
    let private_key = support::require_env("PRIVATE_KEY");
    let market_slug = support::optional_env_with_fallback("MARKET_SLUG", "will-btc-hit-100k");

    let market = sdk.markets.get_market(&market_slug).await?;
    let tokens = support::required_market_tokens(&market)?;

    println!("Market: {}", market.title);
    println!("  YES Token: {}", tokens.yes);
    println!("  NO Token: {}", tokens.no);

    let order_client = sdk.new_order_client(&private_key, None)?;
    println!("Wallet: {}\n", order_client.wallet_address());

    let response = order_client
        .create_order(CreateOrderParams {
            order_type: OrderType::Fok,
            market_slug,
            args: OrderArgs::from(FokOrderArgs {
                token_id: tokens.yes,
                side: Side::Buy,
                maker_amount: 5.0,
                expiration: None,
                nonce: None,
                taker: None,
            }),
            stp_policy: None,
        })
        .await?;

    println!("Order created: {}", response.order.id);
    println!("  Maker Amount: {}", response.order.maker_amount);
    println!("  Taker Amount: {}", response.order.taker_amount);
    if !response.maker_matches.is_empty() {
        println!("  Matched: {} fills", response.maker_matches.len());
    }

    Ok(())
}
