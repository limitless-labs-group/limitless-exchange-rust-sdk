mod support;

use limitless_exchange_rust_sdk::{
    CreateDelegatedOrderParams, FokOrderArgs, OrderArgs, OrderType, Side,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sdk = support::hmac_client()?;
    let market_slug = support::require_env("MARKET_SLUG");
    let target_profile_id = support::optional_positive_i32("LIMITLESS_PARTNER_PROFILE_ID")
        .expect("LIMITLESS_PARTNER_PROFILE_ID environment variable is required");
    let fee_rate_bps = support::optional_positive_i32("LIMITLESS_TARGET_FEE_RATE_BPS")
        .expect("LIMITLESS_TARGET_FEE_RATE_BPS environment variable is required");
    let maker_amount = 1.0;

    let market = sdk.markets.get_market(&market_slug).await?;
    let tokens = support::required_market_tokens(&market)?;

    println!(
        "Submitting delegated FOK BUY order: onBehalfOf={} makerAmount={:.2} USDC",
        target_profile_id, maker_amount
    );

    let response = sdk
        .delegated_orders
        .create_order(CreateDelegatedOrderParams {
            market_slug,
            order_type: OrderType::Fok,
            on_behalf_of: target_profile_id,
            fee_rate_bps,
            args: OrderArgs::from(FokOrderArgs {
                token_id: tokens.yes,
                side: Side::Buy,
                maker_amount,
                expiration: None,
                nonce: None,
                taker: None,
            }),
        })
        .await?;

    println!("Delegated FOK order created: {}", response.order.id);
    if !response.maker_matches.is_empty() {
        println!(
            "Delegated FOK order fully matched with {} fill(s)",
            response.maker_matches.len()
        );
    } else {
        println!("Delegated FOK order was not matched and was cancelled automatically.");
    }

    Ok(())
}
