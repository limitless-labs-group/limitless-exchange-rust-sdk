# Limitless Exchange Rust SDK

**v1.3.0** | Rust SDK parity with the existing Limitless SDK surface

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
limitless-exchange-rust-sdk = "1.3.0"
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

    let profile = sdk.portfolio.get_current_profile().await?;
    println!("Profile: {} {}", profile.id, profile.account);

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

use limitless_exchange_rust_sdk::{Client, GtcOrderArgs, OrderArgs, OrderType, Side};

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

### Optional Receive Window

Order creation can opt into API receive-window freshness checks with `ReceiveWindowOptions`. These values are sent as top-level `POST /orders` fields named `timestamp` and `recvWindow`; they are not included in the signed EIP-712 order payload. Existing `create_order` calls are unchanged and omit both fields.

```rust
use limitless_exchange_rust_sdk::{
    CreateOrderParams, GtcOrderArgs, OrderArgs, OrderType, ReceiveWindowOptions, Side,
};

let order = order_client
    .create_order_with_receive_window(
        CreateOrderParams {
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
        },
        ReceiveWindowOptions {
            timestamp: None,
            recv_window: Some(1500),
        },
    )
    .await?;
```

`recv_window` must be between `1` and `10000` milliseconds. When `recv_window` is supplied without `timestamp`, the SDK stamps the current Unix time in milliseconds. Keep trading hosts NTP-synced; server clock skew tolerance is about one second. Do not retry a `425 Too Early` with the same payload; build a fresh order instead.

### Cancel-Replace Orders

Atomically cancel a resting order and submit its replacement in a single request via `POST /orders/cancel-replace`. Identify the order to cancel with `CancelTarget::OrderId` or `CancelTarget::ClientOrderId`, and set `mode` to `CancelReplaceMode::StopOnFailure` (skip the replacement if the cancel fails) or `CancelReplaceMode::AllowFailure`.

```rust
use limitless_exchange_rust_sdk::{
    CancelReplaceMode, CancelReplaceParams, CancelTarget, GtcOrderArgs, OrderArgs,
    OrderType, ReplacementOrderParams, Side,
};

let result = order_client
    .cancel_replace(CancelReplaceParams {
        cancel: CancelTarget::OrderId { order_id: "old-order-id".to_string() },
        mode: CancelReplaceMode::StopOnFailure,
        on_behalf_of: None,
        replacement: ReplacementOrderParams {
            order_type: OrderType::Gtc,
            market_slug: market.slug.clone(),
            args: OrderArgs::from(GtcOrderArgs {
                token_id: market.outcomes[0].token_id.clone(),
                side: Side::Buy,
                price: 0.5,
                size: 2.0,
                expiration: None,
                nonce: None,
                taker: None,
                post_only: false,
            }),
            client_order_id: None,
            timestamp: None,
            recv_window: None,
            stp_policy: None,
        },
    })
    .await?;
// result.cancel and result.replacement each carry a per-leg status.
```

Replace many orders at once with `order_client.cancel_replace_batch(vec![...])`; the response `results` are index-aligned to the input. Each method has a `*_with_raw` sibling returning `SdkResponse<T>`, and partner integrations use the delegated `cancel_replace` / `cancel_replace_batch` (which set `on_behalf_of`). The single-order variant allows a `409` conflict through as a decodable result.

### Collateral Fee Venues

Some markets trade on collateral fee venues, which charge the taker fee in USDC through a separate on-chain fee module. Existing markets stay on their current venue and behave exactly as before.

- **Detect the venue.** Markets from `get_market` include a `venue`. On collateral fee venues it also carries `fee_module` (`feeModule`) and `fee_module_type == "COLLATERAL"` (`feeModuleType`); legacy venues omit both. Use `venue.is_collateral_fee_venue()` or `is_collateral_fee_venue(market.venue.as_ref())`.
- **Approve USDC to the fee module.** On a collateral fee venue, the wallet that signs orders must call `approve(venue.fee_module, amount)` on the USDC contract (max uint256 recommended) before placing any order that is not `post_only`, for both BUY and SELL. `post_only` orders do not need it. Without it the API rejects the order with `Insufficient collateral allowance for the order fee.` One approval is needed per fee module address; SIMPLE and NegRisk markets use different fee modules, so always read `venue.fee_module` from the market (`venue.collateral_fee_approval_target()` returns it on collateral fee venues and `None` otherwise) and never hardcode it.
- **Existing approvals stay as they are.** Collateral fee venues use the same exchange contracts (and NegRisk adapter) as today, so the existing USDC and conditional-token approvals to `venue.exchange` / `venue.adapter` remain valid, and the EIP-712 order domain (verifying contract = `venue.exchange`) is unchanged.
- **Orders are signed with `fee_rate_bps = 0`.** Any other value is rejected with `FEE_RATE_MISMATCH`. `OrderClient::create_order`, `OrderClient::cancel_replace`, and the delegated order service set this automatically from the market venue (cached by `client.markets.get_market`, otherwise fetched once per market slug); legacy venues keep the current behavior. If you build and sign orders yourself with `OrderBuilder`, `build_unsigned_order`, `sign_order`, or `sign_order_with_config`, set `fee_rate_bps` to `fee_rate_bps_for_venue(market.venue.as_ref(), current_fee_rate_bps)` before signing.
- **Keep a little extra USDC.** BUY orders on collateral fee venues need enough USDC for the order plus the trading fee.
- **Managed wallets need no action.** Server wallets and other wallets Limitless provisions and manages get these approvals automatically.

```rust
let market = client.markets.get_market(&slug).await?;
if let Some(fee_module) = market
    .venue
    .as_ref()
    .and_then(|venue| venue.collateral_fee_approval_target())
{
    // Approve USDC to `fee_module` from the signing wallet (once per fee module address).
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

Use `partner_accounts.list_accounts` to list partner-owned sub-accounts, or pass `account` to recover a specific child profile by address. This endpoint requires scoped HMAC credentials with the `account_creation` scope. The API caps `limit` at 25 and rejects `x-on-behalf-of` on this route.

```rust
use limitless_exchange_rust_sdk::{Client, HmacCredentials, ListPartnerAccountsParams};

let sdk = Client::from_http_client(
    Client::builder()
        .hmac_credentials(HmacCredentials {
            token_id: std::env::var("LIMITLESS_API_TOKEN_ID")?,
            secret: std::env::var("LIMITLESS_API_TOKEN_SECRET")?,
        })
        .build()?,
)?;

let accounts = sdk
    .partner_accounts
    .list_accounts(&ListPartnerAccountsParams {
        limit: Some(25),
        page: Some(1),
        ..Default::default()
    })
    .await?;

let recovered = sdk
    .partner_accounts
    .list_accounts(&ListPartnerAccountsParams {
        account: Some("0xChildAccount".to_string()),
        limit: Some(25),
        page: Some(1),
    })
    .await?;

println!("accounts: {} recovered: {}", accounts.data.len(), recovered.data.len());
```

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

### Server Wallet Redeem & Withdraw

Use `server_wallets.redeem_positions` and `server_wallets.withdraw` only for server-managed wallets created in delegated-signing partner flows with `create_server_wallet = true`.

- `redeem_positions` calls `POST /portfolio/redeem`
- `withdraw` calls `POST /portfolio/withdraw`
- both operations require HMAC-scoped API-token auth
- `withdraw` also requires the `withdrawal` scope
- set `on_behalf_of` to the delegated child-profile id when withdrawing child server-wallet funds
- omit `on_behalf_of` only when withdrawing the authenticated caller's own server wallet to an explicit `destination`
- omit `destination` to use the API default: authenticated partner smart wallet when present, otherwise authenticated partner account
- pass `destination` to withdraw directly to the authenticated partner account, authenticated partner smart wallet, or an active withdrawal address allowlisted on the authenticated partner profile
- `partner_accounts.add_withdrawal_address` and `partner_accounts.delete_withdrawal_address` manage the allowlist with Privy identity-token auth; API-token auth is not used for those allowlist endpoints

```rust
use limitless_exchange_rust_sdk::{
    Client, HmacCredentials, PartnerWithdrawalAddressInput, WithdrawServerWalletParams,
};

let sdk = Client::from_http_client(
    Client::builder()
        .hmac_credentials(HmacCredentials {
            token_id: std::env::var("LIMITLESS_API_TOKEN_ID")?,
            secret: std::env::var("LIMITLESS_API_TOKEN_SECRET")?,
        })
        .build()?,
)?;

let identity_token = std::env::var("LIMITLESS_IDENTITY_TOKEN")?;
let treasury_address = "0x0F3262730c909408042F9Da345a916dc0e1F9787";

sdk.partner_accounts
    .add_withdrawal_address(
        &identity_token,
        &PartnerWithdrawalAddressInput {
            address: treasury_address.to_string(),
            label: Some("treasury".to_string()),
        },
    )
    .await?;

let child_withdraw = sdk
    .server_wallets
    .withdraw(&WithdrawServerWalletParams {
        amount: "5000000".to_string(),
        on_behalf_of: Some(352),
        token: None,
        destination: Some(treasury_address.to_string()),
    })
    .await?;

let own_wallet_withdraw = sdk
    .server_wallets
    .withdraw(&WithdrawServerWalletParams {
        amount: "5000000".to_string(),
        on_behalf_of: None,
        token: None,
        destination: Some(treasury_address.to_string()),
    })
    .await?;

println!(
    "withdraws: {} {}",
    child_withdraw.envelope.transaction_id, own_wallet_withdraw.envelope.transaction_id
);
```

### Partner AMM Trading

`sdk.amm` submits AMM market approvals and exact-collateral buy/sell orders for partner server wallets.

- `check_allowance` / `approve_allowance` call `POST /amm/allowances/{check,approve}`
- `buy` calls `POST /amm/buy`, `sell` calls `POST /amm/sell`
- `ensure_allowance` runs check → approve-once → poll `check` until confirmed (default 2s interval, 30 attempts, configurable via `AmmAllowancePollOptions`)
- all four endpoints require HMAC-scoped API-token auth (a token holding **both** `trading` and `delegated_signing` scopes) or an explicit Privy identity token via the `*_with_identity` methods; **legacy API keys are rejected**
- `outcome_index` is `0` (YES) or `1` (NO); collateral amounts are positive integer **strings** in the collateral token's base units (never floats)
- `idempotency_key` is required per trade and retained by the API for 24 hours; retry timed-out trades with the **exact same params** so the serialized body stays byte-identical
- buy/sell never check or submit allowances themselves — confirm the allowance first
- set `on_behalf_of` to a sub-account profile id (`1..=2147483647`); omit it (`None`) for the authenticated profile

```rust
use limitless_exchange_rust_sdk::{
    AmmAllowanceParams, AmmAllowanceSide, AmmBuyParams, Client, HmacCredentials,
};

let sdk = Client::from_http_client(
    Client::builder()
        .hmac_credentials(HmacCredentials {
            token_id: std::env::var("LIMITLESS_API_TOKEN_ID")?,
            secret: std::env::var("LIMITLESS_API_TOKEN_SECRET")?,
        })
        .build()?,
)?;

// 1. Confirm the BUY allowance (check -> approve-once -> poll check).
let allowance = sdk
    .amm
    .ensure_allowance(
        &AmmAllowanceParams {
            market: "btc-above-100k".to_string(),
            side: AmmAllowanceSide::Buy,
            on_behalf_of: None,
        },
        None,
    )
    .await?;

// 2. Submit an exact-collateral buy (no allowance preflight inside buy).
if allowance.confirmed {
    let buy = sdk
        .amm
        .buy(&AmmBuyParams {
            market: "btc-above-100k".to_string(),
            outcome_index: 0,
            collateral_amount: "1000000".to_string(),
            slippage_bps: Some(100),
            idempotency_key: "unique-key-per-trade".to_string(),
            on_behalf_of: None,
        })
        .await?;
    println!("buy status={:?} minShares={}", buy.status, buy.min_shares);
}
```

### Raw HTTP Responses

Every API-backed service method has a `*_with_raw` sibling that returns `SdkResponse<T>` instead of just `T`:

```rust
pub struct SdkResponse<T> {
    pub data: T,          // identical to what the base method returns
    pub raw: RawResponse, // { status: u16, headers: HeaderMap, body: Vec<u8> }
}
```

Use the raw variant when you need the HTTP status code, response headers, or the exact response bytes; the base methods are unchanged and still return `T`. Rust cannot overload on return type, so raw mode is an explicit method rather than a flag.

```rust
// Base method — decoded value only.
let markets = sdk.markets.get_active_markets(None).await?;

// Raw sibling — decoded value plus the underlying HTTP response.
let response = sdk.markets.get_active_markets_with_raw(None).await?;
println!("HTTP {}", response.raw.status);
println!("{} markets", response.data.data.len());

// Available across services, including trades:
let buy = sdk.amm.buy_with_raw(&buy_params).await?;
println!("HTTP {} — {:?}", buy.raw.status, buy.data.status);
```

Raw mode still returns typed errors (`LimitlessError::Api`, carrying the numeric `status`) for HTTP status codes ≥ 400.


## Partner curated Packs

The Packs service supports catalogue list/search/detail, HMAC quote and buy,
managed position list/detail, mint polling, claim progression and claim receipt
confirmation. Partner calls require `trading` and `delegated_signing` scopes.
Use a funded managed server wallet. Buy amounts are integer micro-USDC strings;
position amounts may include trailing decimal zeros in the same units.
Before quoting/buying, check `GET /profiles/partner-accounts/:id/allowances` using
the partner token. Find the USDC target labelled `parlay-treasury-vault` and
require `confirmed: true` with `status: confirmed`; overall `ready` alone does
not establish that the vault target exists. If missing, call
`POST /profiles/partner-accounts/:id/allowances/retry`, then poll the GET until
confirmed with a bounded timeout. `submitted` is not confirmation. Stop on an
absent vault target or failed repair. These endpoints additionally require
`account_creation` and use the backend's existing unlimited-approval policy;
repair may approve other missing trading targets too. They are available through
the SDK's partner-account service. Packs do not invoke repair automatically.
Code-bearing wallets need a standing vault allowance. EOAs may use a supported,
verified permit when allowance is insufficient; permit support is not universal.

Persist the complete buy request before submission. Retry with the **same body**,
including the original deadline and idempotency key, even after expiry. The SDK
never generates or rotates keys, and submits each write once. Buy `PENDING` and
`RECOVERING` mean keep the existing key and wait for minting. A **position** with
status `FAILED` can be reactivated by backend reconciliation, so mint polling
continues through it. Buy-status `FAILED` corresponds to position `VAULT_REVERTED`,
a confirmed vault revert. Never create a new purchase merely because a position
says `FAILED` or polling times out. On mint polling exhaustion, replay the identical
saved buy: that response distinguishes explicit `NOT_SUBMITTED` from recovery.
Transport failures and generic 5xx errors remain ambiguous; retain the request.

Reads set `x-on-behalf-of` per request; writes use body `onBehalfOf`. Do not set
a global delegation header. Reuse one Packs service per partner: calls are
serialized and spaced 1.1 seconds apart. After HTTP 429, subsequent calls wait
at least 10 seconds. Separate clients/processes must coordinate the shared
partner-owner limit themselves. Claim polling stops on API errors, especially
`PACK_SUBMISSION_UNKNOWN`; recover using a known mined redemption hash and
claim confirmation. A `userOperationHash` is not a transaction hash.
Claim helpers deliberately stop on all API errors. After a 429, back off at least
10 seconds; after the exact wallet-busy conflict, wait for the lock to clear;
after `PACK_TRANSACTION_REVERTED`, inspect the reported revert. Call claim again
with the same position to resume these retryable cases. There are no hidden
transaction retries, and `PACK_SUBMISSION_UNKNOWN` always requires investigation.

Polling checks at five-second intervals, with 60 attempts by default, and supports
cancellation. Waiting for a position means waiting for minting, not settlement.
Packs response bodies are omitted from HTTP logs to avoid exposing unexpected
prepared transaction material. Deployment requires the partner-Pack migration,
an active catalogue and configured treasury/vault.

`classify_pack_error(&error)` returns typed `PackErrorDetails` without modifying the original
API error. Import it from the package root. `kind`/`Kind` identifies the cases
below (Rust uses the corresponding `PackErrorKind` enum variants). Optional
payload fields expose `code`, `submissionState`, `parlayId`, `recovery`,
`tokenAddress`, `spender`, and `requiredAllowance` using each language's naming
conventions. Unknown errors classify as `OTHER`; that never proves no submission.

| Kind | Caller recovery |
| --- | --- |
| `ALLOWANCE_REQUIRED` | HTTP 409 `PACK_ALLOWANCE_REQUIRED`: repair the specified vault allowance and wait for confirmation, then replay the saved buy. Do not rotate its key just for approval. |
| `NOT_SUBMITTED` | Explicit 4xx `submissionState: NOT_SUBMITTED`: fix the reported cause, then obtain a fresh quote/deadline and persist a new key. The old key stays bound to its failed position. |
| `PRICE_DRIFTED` | HTTP 409 `PRICE_DRIFTED`: obtain a fresh quote and persist a new purchase key. |
| `INTENT_EXPIRED` | Only HTTP 401 with exact message `Intent expired — please retry`: obtain a fresh quote/deadline/key. Local expiry or another 401 is insufficient evidence. |
| `WALLET_BUSY` | Wait and retry the identical request. |
| `RATE_LIMITED` | Back off at least 10 seconds; retry the identical request. |
| `SUBMISSION_UNKNOWN` | Stop and investigate; confirm a known mined redemption receipt for claims. |
| `TRANSACTION_REVERTED` | Claim step reverted; inspect cause before resuming the same position. |

An ineligible quote may contain only `eligible: false` and `ineligibleReasons`.
Check eligibility before accessing optional quote pricing/leg fields.

```rust,ignore
let quote = client.packs.quote(&PackQuoteParams {
    pack_id: pack_id.clone(), stake_amount: "1000000".into(),
}).await?;
if !quote.eligible { return Err(LimitlessError::invalid_input("Pack is ineligible")); }
let payout = quote.potential_payout.as_deref()
    .ok_or_else(|| LimitlessError::invalid_input("Eligible quote omitted payout"))?;
let request = PackBuyParams {
    on_behalf_of: profile_id, pack_id, stake_amount: "1000000".into(),
    min_payout_amount: pack_min_payout(payout, 100)?,
    deadline: (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .expect("clock").as_secs() + 120).to_string(),
    idempotency_key: durable_order_id,
};
// Save serde_json::to_string(&request)? durably before this call.
let result = client.packs.buy(&request).await?;
```

Use `wait_for_position`, `claim_once`, `claim`, and `confirm_claim`.
`PackPollOptions::default()` supplies bounded polling; dropping the future
cancels it. Cloned Packs services share their request queue and pacing.
