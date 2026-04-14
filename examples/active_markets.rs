mod support;

use limitless_exchange_rust_sdk::{ActiveMarketsParams, ActiveMarketsSortBy};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sdk = support::public_client()?;

    let response = sdk
        .markets
        .get_active_markets(Some(&ActiveMarketsParams {
            limit: Some(5),
            page: None,
            sort_by: Some(ActiveMarketsSortBy::Newest),
        }))
        .await?;

    println!("Total markets: {}\n", response.total_markets_count);

    for market in response.data {
        println!("Market: {}", market.title);
        println!("  Slug: {}", market.slug);
        println!("  Status: {}", market.status);
        println!("  Trade Type: {}", market.trade_type);
        if !market.prices.is_empty() {
            println!("  Prices: {:?}", market.prices);
        }
        println!();
    }

    Ok(())
}
