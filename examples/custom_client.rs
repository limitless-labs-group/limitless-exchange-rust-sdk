mod support;

use std::{collections::HashMap, sync::Arc, time::Duration};

use limitless_exchange_rust_sdk::{
    ActiveMarketsParams, ActiveMarketsSortBy, Client, ConsoleLogger, LogLevel,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let base_url =
        support::optional_env_with_fallback("LIMITLESS_BASE_URL", "https://api.limitless.exchange");
    let mut additional_headers = HashMap::new();

    if let Some(strategy_name) = support::optional_env("LIMITLESS_STRATEGY_HEADER") {
        additional_headers.insert("X-Strategy-Name".to_string(), strategy_name);
    }

    let http = Client::builder()
        .base_url(base_url)
        .timeout(Duration::from_secs(20))
        .additional_headers(additional_headers)
        .logger(Arc::new(ConsoleLogger::new(LogLevel::Debug)))
        .build()?;
    let sdk = Client::from_http_client(http)?;

    let markets = sdk
        .markets
        .get_active_markets(Some(&ActiveMarketsParams {
            limit: Some(5),
            page: None,
            sort_by: Some(ActiveMarketsSortBy::Liquidity),
        }))
        .await?;

    println!(
        "Fetched {} markets with a custom-configured client.",
        markets.data.len()
    );
    for market in markets.data.iter().take(5) {
        println!("- {} ({})", market.title, market.slug);
    }

    Ok(())
}
