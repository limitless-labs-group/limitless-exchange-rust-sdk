mod support;

use limitless_exchange_rust_sdk::{
    CreateDelegatedOrderParams, GtcOrderArgs, OrderArgs, OrderType, Side,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sdk = support::hmac_client()?;
    let market_slug = support::require_env("MARKET_SLUG");
    let target_profile_id = support::optional_positive_i32("LIMITLESS_PARTNER_PROFILE_ID")
        .expect("LIMITLESS_PARTNER_PROFILE_ID environment variable is required");
    let fee_rate_bps = support::optional_positive_i32("LIMITLESS_TARGET_FEE_RATE_BPS")
        .expect("LIMITLESS_TARGET_FEE_RATE_BPS environment variable is required");

    let market = sdk.markets.get_market(&market_slug).await?;
    let tokens = support::required_market_tokens(&market)?;

    let response = sdk
        .delegated_orders
        .create_order(CreateDelegatedOrderParams {
            market_slug,
            order_type: OrderType::Gtc,
            on_behalf_of: target_profile_id,
            fee_rate_bps,
            args: OrderArgs::from(GtcOrderArgs {
                token_id: tokens.yes,
                side: Side::Buy,
                price: 0.050,
                size: 1.0,
                expiration: None,
                nonce: None,
                taker: None,
                post_only: true,
            }),
        })
        .await?;

    println!("Delegated order created: {}", response.order.id);
    Ok(())
}
