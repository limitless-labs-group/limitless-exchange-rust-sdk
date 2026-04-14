# Limitless Exchange Rust SDK

**v1.0.6** | Rust SDK parity with the existing Limitless SDK surface

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

## Status

This is the first full-surface parity pass. The crate is implemented against the Go SDK shape and verified locally with:

- `cargo fmt`
- `cargo check --examples`
- `cargo test`

## Installation

```toml
[dependencies]
limitless-exchange-rust-sdk = "1.0.6"
```

## Example

```rust
use limitless_exchange_rust_sdk::{Client, OrderArgs, OrderType, Side, GtcOrderArgs};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::from_http_client(
        Client::builder()
            .api_key("your-api-key")
            .build()?
    )?;

    let market = client.markets.get_market("btc-above-150k-by-jun-2026").await?;
    println!("market: {}", market.title);

    let order_client = client.new_order_client(
        "0xYOUR_PRIVATE_KEY",
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

## Examples

The repository includes the same example catalog as the Go SDK under [examples/](examples/):

- `cargo run --example active_markets`
- `cargo run --example portfolio`
- `cargo run --example api_tokens`
- `cargo run --example clob_gtc_order`
- `cargo run --example clob_fak_order`
- `cargo run --example clob_fok_order`
- `cargo run --example negrisk_order`
- `cargo run --example delegated_order`
- `cargo run --example delegated_fok_order`
- `cargo run --example e2e_fok_flow`
- `cargo run --example server_wallet_redeem_withdraw`
- `cargo run --example websocket_orderbook`
- `cargo run --example websocket_positions`

See [examples/README.md](examples/README.md) for required environment variables and per-example notes.
