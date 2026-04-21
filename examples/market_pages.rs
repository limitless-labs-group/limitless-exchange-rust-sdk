mod support;

use std::collections::HashMap;

use limitless_exchange_rust_sdk::{MarketPageMarketsParams, MarketPageSort};
use serde_json::Value;

fn add_filter_from_env(filters: &mut HashMap<String, Value>, query_key: &str, env_key: &str) {
    if let Some(value) = support::optional_env(env_key) {
        filters.insert(query_key.to_string(), Value::String(value));
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sdk = support::public_client()?;
    let page_path = support::optional_env_with_fallback("MARKET_PAGE_PATH", "/crypto");

    let navigation = sdk.pages.get_navigation().await?;
    println!("Top-level navigation nodes: {}", navigation.len());
    for node in navigation.iter().take(5) {
        println!(
            "- {} ({}) children={}",
            node.name,
            node.path,
            node.children.len()
        );
    }

    let page = sdk.pages.get_market_page_by_path(&page_path).await?;
    println!(
        "\nResolved page: name={} id={} fullPath={}",
        page.name, page.id, page.full_path
    );
    println!("Filter groups on page: {}", page.filter_groups.len());

    let mut filters = HashMap::new();
    add_filter_from_env(&mut filters, "ticker", "MARKET_PAGE_TICKER_FILTER");
    add_filter_from_env(&mut filters, "duration", "MARKET_PAGE_DURATION_FILTER");

    let response = sdk
        .pages
        .get_markets(
            &page.id,
            Some(&MarketPageMarketsParams {
                page: None,
                limit: Some(5),
                sort: Some(MarketPageSort::UpdatedAtDesc),
                cursor: Some(String::new()),
                filters,
            }),
        )
        .await?;

    println!("\nMarkets returned: {}", response.data.len());
    for market in response.data.iter().take(5) {
        println!(
            "- {} slug={} tradeType={} status={}",
            market.title, market.slug, market.trade_type, market.status
        );
    }

    if let Some(next_cursor) = response
        .cursor
        .as_ref()
        .and_then(|cursor| cursor.next_cursor.as_deref())
    {
        let next_page = sdk
            .pages
            .get_markets(
                &page.id,
                Some(&MarketPageMarketsParams {
                    page: None,
                    limit: Some(5),
                    sort: Some(MarketPageSort::UpdatedAtDesc),
                    cursor: Some(next_cursor.to_string()),
                    filters: HashMap::new(),
                }),
            )
            .await?;
        println!("Next cursor page returned {} markets", next_page.data.len());
    } else if let Some(pagination) = response.pagination.as_ref() {
        println!(
            "Offset pagination: page={} total={} totalPages={}",
            pagination.page, pagination.total, pagination.total_pages
        );
    }

    let property_keys = sdk.pages.get_property_keys().await?;
    println!("\nProperty keys available: {}", property_keys.len());
    if let Some(first_key) = property_keys.first() {
        let detailed_key = sdk.pages.get_property_key(&first_key.id).await?;
        println!(
            "First property key: {} slug={} type={}",
            detailed_key.name, detailed_key.slug, detailed_key.property_type
        );

        let options = sdk
            .pages
            .get_property_options(&detailed_key.id, None)
            .await?;
        println!("Options for {}: {}", detailed_key.slug, options.len());
        for option in options.iter().take(5) {
            println!("- {} ({})", option.label, option.value);
        }
    }

    Ok(())
}
