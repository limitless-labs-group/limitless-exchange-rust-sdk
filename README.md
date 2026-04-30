# Limitless Exchange Rust SDK

**v1.0.9** | Rust SDK parity with the existing Limitless SDK surface

Rust SDK for interacting with the Limitless Exchange API.

This crate is a parity-driven Rust port of the existing Limitless SDK surface. The current implementation includes:

- shared HTTP client with API key, identity-header, and HMAC auth support
- typed API errors and retry helpers
- root `Client`
- markets, portfolio, and market-pages services
- partner api-token, partner-account, and server-wallet services
- order builder, validator, EIP-712 signer, and order client
- delegated-order service
- websocket types and socket.io client surface

**USE AT YOUR OWN RISK**

This SDK is provided "as-is" without any warranties or guarantees. Trading on prediction markets involves financial risk. By using this SDK, you acknowledge that:

- You are responsible for testing the SDK thoroughly before using it in production
- The SDK authors are not liable for any financial losses or damages
- You should review and understand the code before executing any trades
- It is recommended to test all functionality on testnet or with small amounts first
- The SDK may contain bugs or unexpected behavior despite best efforts

**ALWAYS TEST BEFORE USING IN PRODUCTION WITH REAL FUNDS**

For production use, we strongly recommend:

1. Running comprehensive tests with your specific use case
2. Starting with small transaction amounts
3. Monitoring all transactions carefully
4. Having proper error handling and recovery mechanisms

## Geographic Restrictions

**Important**: Limitless restricts order placement from US locations due to regulatory requirements and compliance with international sanctions. Before placing orders, builders should verify their location complies with applicable regulations.

## Status

This is the first full-surface parity pass. The crate is implemented against the Go SDK shape and verified locally with:

- `cargo fmt`
- `cargo check --examples`
- `cargo test`

## Installation

```toml
[dependencies]
limitless-exchange-rust-sdk = "1.0.9"
```

## Authentication Modes

- Public read-only endpoints: no authentication required. Use these for active markets, market pages, and orderbooks.
- API key authentication: required for portfolio and standard order-placement flows.
- HMAC-scoped authentication: used for partner/delegated/server-wallet flows and can also authenticate websocket position streams.

The SDK reads `LIMITLESS_API_KEY` automatically when present, or you can configure credentials explicitly with `Client::builder()`.

## Quick Start

### Public Market Data

```rust
use limitless_exchange_rust_sdk::{ActiveMarketsParams, ActiveMarketsSortBy, Client};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sdk = Client::new()?;

    let markets = sdk
        .markets
        .get_active_markets(Some(&ActiveMarketsParams {
            limit: Some(5),
            page: None,
            sort_by: Some(ActiveMarketsSortBy::Newest),
        }))
        .await?;

    println!("Found {} markets", markets.data.len());
    Ok(())
}
```

### Authenticated Portfolio Access

```rust
use std::env;

use limitless_exchange_rust_sdk::Client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sdk = Client::from_http_client(
        Client::builder()
            .api_key(env::var("LIMITLESS_API_KEY")?)
            .build()?,
    )?;

    let positions = sdk.portfolio.get_positions().await?;
    println!("CLOB positions: {}", positions.clob.len());

    let history = sdk.portfolio.get_user_history(None, Some(20)).await?;
    println!("History entries: {}", history.data.len());

    if let Some(next_cursor) = history.next_cursor.as_deref() {
        let next_page = sdk
            .portfolio
            .get_user_history(Some(next_cursor), Some(20))
            .await?;
        println!("Next page entries: {}", next_page.data.len());
    }

    Ok(())
}
```

### Signed Order Placement

```rust
use std::env;

use limitless_exchange_rust_sdk::{Client, OrderArgs, OrderType, Side, GtcOrderArgs};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::from_http_client(
        Client::builder()
            .api_key(env::var("LIMITLESS_API_KEY")?)
            .build()?
    )?;

    let market = client.markets.get_market("btc-above-150k-by-jun-2026").await?;
    println!("market: {}", market.title);

    let private_key = env::var("PRIVATE_KEY")?;
    let order_client = client.new_order_client(
        &private_key,
        None,
    )?;

    let order = order_client
        .create_order(limitless_exchange_rust_sdk::CreateOrderParams {
            order_type: OrderType::Gtc,
            market_slug: market.slug.clone(),
            args: OrderArgs::from(GtcOrderArgs {
                token_id: market.outcomes[0].token_id.clone(),
                side: Side::Buy,
                price: 0.51,
                size: 10.0,
                expiration: None,
                nonce: None,
                taker: None,
                post_only: false,
            }),
        })
        .await?;

    println!("order id: {}", order.order.id);
    Ok(())
}
```

## Workflow Guide

- Public market discovery: [examples/active_markets.rs](examples/active_markets.rs)
- Custom client builder and logging: [examples/custom_client.rs](examples/custom_client.rs)
- Market-page discovery and filtered browsing: [examples/market_pages.rs](examples/market_pages.rs)
- Portfolio and cursor-based history: [examples/portfolio.rs](examples/portfolio.rs)
- User-order retrieval and market cancel-all: [examples/user_orders.rs](examples/user_orders.rs)
- Signed CLOB orders:
  - GTC: [examples/clob_gtc_order.rs](examples/clob_gtc_order.rs)
  - FAK: [examples/clob_fak_order.rs](examples/clob_fak_order.rs)
  - FOK: [examples/clob_fok_order.rs](examples/clob_fok_order.rs)
- NegRisk order flow: [examples/negrisk_order.rs](examples/negrisk_order.rs)
- Delegated partner flows:
  - delegated order: [examples/delegated_order.rs](examples/delegated_order.rs)
  - delegated FOK order: [examples/delegated_fok_order.rs](examples/delegated_fok_order.rs)
- Partner allowance recovery: [examples/partner_account_allowances.rs](examples/partner_account_allowances.rs)
- API-token revoke flow: [examples/api_token_revoke.rs](examples/api_token_revoke.rs)
- Server-wallet redeem/withdraw flow: [examples/server_wallet_redeem_withdraw.rs](examples/server_wallet_redeem_withdraw.rs)
- WebSocket subscriptions:
  - orderbook: [examples/websocket_orderbook.rs](examples/websocket_orderbook.rs)
  - positions and transactions: [examples/websocket_positions.rs](examples/websocket_positions.rs)

### Partner Server-Wallet Allowances

Use `partner_accounts.check_allowances(profile_id)` and `partner_accounts.retry_allowances(profile_id)` only for partner child profiles created with `create_server_wallet = true`. These endpoints require scoped HMAC credentials derived with `SCOPE_ACCOUNT_CREATION` and `SCOPE_DELEGATED_SIGNING`.

```rust
use limitless_exchange_rust_sdk::{Client, HmacCredentials};

let sdk = Client::from_http_client(
    Client::builder()
        .hmac_credentials(HmacCredentials {
            token_id: std::env::var("LIMITLESS_API_TOKEN_ID")?,
            secret: std::env::var("LIMITLESS_API_TOKEN_SECRET")?,
        })
        .build()?,
)?;

let profile_id = 12345;
let mut allowances = sdk.partner_accounts.check_allowances(profile_id).await?;
if !allowances.ready {
    // Retry re-checks live chain state and submits only targets still missing.
    // A returned "submitted" status means this request submitted a sponsored tx/user operation.
    allowances = sdk.partner_accounts.retry_allowances(profile_id).await?;
}

println!("allowance ready: {}", allowances.ready);
```

Poll `check_allowances` first. If `ready` is false and one or more targets are `missing` or `failed` with `retryable = true`, call `retry_allowances`, then poll `check_allowances` again after a short delay. Retry `429` and `409` responses are returned as `LimitlessError::Api`; inspect `err.status`, and for `429` read `retryAfterSeconds` from `err.data`.
