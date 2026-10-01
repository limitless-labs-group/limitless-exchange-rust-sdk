mod support;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sdk = support::api_key_client()?;
    let market_slug = support::optional_env_with_fallback("MARKET_SLUG", "will-btc-hit-100k");

    let direct_orders = sdk.markets.get_user_orders(&market_slug).await?;
    println!(
        "Direct user-order lookup for {} returned {} order(s)",
        market_slug,
        direct_orders.len()
    );

    let market = sdk.markets.get_market(&market_slug).await?;
    let fluent_orders = market.get_user_orders().await?;
    println!(
        "Fluent market.get_user_orders() returned {} order(s)",
        fluent_orders.len()
    );

    for order in fluent_orders.iter().take(10) {
        println!(
            "- id={} status={} type={} side={} price={} remainingSize={}",
            order.id, order.status, order.order_type, order.side, order.price, order.remaining_size
        );
    }

    if !support::env_flag("LIMITLESS_CANCEL_ALL_ORDERS", false) {
        println!(
            "\nSkipping cancel-all. Re-run with LIMITLESS_CANCEL_ALL_ORDERS=1 and PRIVATE_KEY set to cancel all live orders for this market."
        );
        return Ok(());
    }

    let private_key = support::require_env("PRIVATE_KEY");
    let order_client = sdk.new_order_client(&private_key, None)?;
    let message = order_client.cancel_all(&market_slug).await?;
    println!("\nCancel-all response: {}", message);

    Ok(())
}
