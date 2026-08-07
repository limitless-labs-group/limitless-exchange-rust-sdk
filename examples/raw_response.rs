mod support;

/// Demonstrates opt-in raw HTTP responses.
///
/// Every API-backed service method has a `*_with_raw` sibling that returns an
/// `SdkResponse<T> { data, raw }`: `data` is the same decoded value the base
/// method returns, and `raw` exposes the HTTP status code, response headers, and
/// the exact response bytes. The base methods are unchanged.
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sdk = support::public_client()?;

    // GET /markets/active with the raw response surfaced.
    let response = sdk.markets.get_active_markets_with_raw(None).await?;

    println!("HTTP {}", response.raw.status);
    if let Some(content_type) = response
        .raw
        .headers
        .get("content-type")
        .and_then(|value| value.to_str().ok())
    {
        println!("content-type: {content_type}");
    }
    println!("response body: {} bytes", response.raw.body.len());
    println!(
        "decoded {} active markets (totalMarketsCount={})",
        response.data.data.len(),
        response.data.total_markets_count
    );

    // The base method returns just the decoded value, unchanged.
    let markets = sdk.markets.get_active_markets(None).await?;
    println!("base method returned {} markets", markets.data.len());

    Ok(())
}
