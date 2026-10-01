# Changelog

All notable changes to the Limitless Exchange Rust SDK will be documented in this file.

## [Unreleased]

### Breaking

Three of the fixes below change the type of a field that 1.1.0 decoded successfully, so code that reads them stops compiling: `MarketMetadata::fee` (`bool` to `Option<bool>`), `PriceOracleMetadata::pyth_address` (`String` to `Option<String>`) and `PortfolioPositionsResponse::points` / `accumulative_points` (`Option<String>` to `Option<f64>`). `ActiveMarketsSortBy::Liquidity` is removed (it was always rejected by the server), and `UserOrder` and `NewPriceData` are reshaped to the wire (neither decoded a non-empty response on 1.1.0, so no working code depends on the old shapes). Under Cargo semver this is a major bump; whether to release as 2.0.0 or stay on 1.x with this callout is the maintainers' call.

### Fixed

- `PriceOracleMetadata::pyth_address` is now `Option<String>` (with `#[serde(default)]`). Hourly and daily up-or-down replicas charted from Binance or Pyth Pro send `pythAddress: null`, which failed deserialization of the whole `Market` and therefore of the default `get_active_markets(None)` page and of `get_market(slug)` for those markets.
- `UserOrder` (`GET /markets/{slug}/user-orders`) now matches the matching-engine row the server sends: `id`, `owner_id`, `market_id` (text), `token`, `order_type` (wire `type`), `status`, `side`, decimal-string `maker_amount` / `taker_amount` / `price` / `original_size` / `remaining_size`, `created_at`, and optional `client_order_id`. The previous signed-order shape (`signature_type`, `salt`, `maker`, `signer`, `taker`, `token_id`, `fee_rate_bps`, `nonce`, `signature`, `filled_size`, numeric `market_id`) never deserialized a non-empty response.
- `PortfolioPositionsResponse::points` / `accumulative_points` are now `Option<f64>` and accept both the decimal string and the JSON number `0` the server sends for profiles without a points row (previously `Option<String>`, which failed on the number and took `get_positions()` down).
- `NewPriceData` now decodes both `newPriceData` shapes: the initial per-market snapshot (top-level `marketAddress`, single `updatedPrices` object) and streamed updates (no top-level `marketAddress`, `updatedPrices` array). `market_address` is `Option<String>`, `AmmPriceEntry::market_id` is `Option<i32>`, and `updated_prices` stays a `Vec`.
- `MarketMetadata::fee` is now `Option<bool>` (the API allows `null` and empty metadata), and every other metadata key (sports and venue fields) is preserved in the new `extra` map instead of being dropped.
- Cancel-replace response types (`CancelReplaceCancelResult`, `CancelReplaceSuccessData`, `CancelReplaceNotAttempted`, `CancelReplaceReplacementResult`) no longer use `deny_unknown_fields`, so additive server fields do not fail the decode. They were the only strict response types in the crate; the request-side `CancelTarget` keeps `deny_unknown_fields`.

### Changed

- `ActiveMarketsSortBy::Liquidity` is replaced by `ActiveMarketsSortBy::Trending` (`sortBy=trending`). The server never accepted `liquidity` (`GET /markets/active?sortBy=liquidity` returns HTTP 400); its sort options are `ending_soon`, `high_value`, `lp_rewards`, `newest` and `trending`.

### Added

- `MarketSettings::{taker_delay_ms, creator_fee_pct, effective_daily_reward, current_rewards_multiplier}`, matching the keys every `/markets/active` row carries. `taker_delay_ms` is the only way to know ahead of time that a taker order will come back `DELAYED`.
- `PriceOracleMetadata::chart_source` (`chainlink`, `pyth`, `pyth-pro` or `binance`).
- `OrderBook::midpoint` (the raw midpoint next to `adjusted_midpoint`).

## [1.1.0]

### Added

- Partner AMM trading via the new `AmmService` (exposed as `Client::amm`):
  - `check_allowance` / `approve_allowance` (`POST /amm/allowances/{check,approve}`), `buy` (`POST /amm/buy`), `sell` (`POST /amm/sell`), and the `ensure_allowance` helper (check → approve-once → poll `check` with a 2s default interval and 30-attempt default, configurable via `AmmAllowancePollOptions`).
  - HMAC-scoped API-token auth (token must hold both `trading` and `delegated_signing` scopes) or explicit Privy identity auth via `*_with_identity` variants; legacy API keys are rejected before any network call (`AMM_HMAC_ONLY_ERROR`).
  - Typed requests/responses with camelCase wire mapping: `AmmAllowanceParams`, `AmmBuyParams`, `AmmSellParams`, `AmmAllowanceResponse`, `AmmBuyResponse`, `AmmSellResponse`, `AmmTransactionIdentifiers` (flattened, independently-optional `transactionId`/`userOperationHash`/`txHash`), and the `AmmAllowanceSide`, `AmmAllowanceStatus`, and `AmmTradeStatus` enums.
  - SDK-side validation returning `LimitlessError::InvalidInput` before the network: non-empty market ≤255 chars, `outcome_index ∈ {0,1}`, positive-integer-string amounts (`^[1-9]\d*$`, ≤78 chars, ≤ uint256 max), `slippage_bps` 0..1000, non-blank `idempotency_key` ≤128 chars, and `on_behalf_of` in `1..=2147483647` (omitted from the wire when `None`).
  - AMM error mapping stays on the existing `LimitlessError::Api(ApiError)` model — match on the numeric `.status` (403/409/422/425/429/502/503) and read `.data`; no new error enum variants were added.
  - Added the runnable `amm_trading` example.
- Opt-in raw HTTP responses across all API-backed service methods. Each `foo(...) -> Result<T>` now has a sibling `foo_with_raw(...) -> Result<SdkResponse<T>>` exposing the underlying `RawResponse` (`status`, `headers`, `body`) alongside the decoded value; the base methods delegate to the raw variants and are behavior-preserving.
  - New generic wrapper `SdkResponse<T> { data: T, raw: RawResponse }` (re-exported from the crate root).
  - New transport primitives `HttpClient::{post_raw, patch_raw, delete_raw, post_raw_with_identity, post_raw_with_headers, get_raw_with_identity, delete_raw_with_identity}` with matching `RetryableClient::{post_raw, patch_raw, delete_raw}` pass-throughs.
  - Coverage spans `markets`, `market_pages`, `portfolio`, `api_tokens`, `partner_accounts`, `delegated_orders`, `server_wallets`, `orders`, and the new `amm` service.
  - Added the runnable `raw_response` example.
- Atomic cancel-replace for orders via `OrderClient::cancel_replace` / `cancel_replace_batch` (with `cancel_replace_with_raw` / `cancel_replace_batch_with_raw` siblings) and the delegated `cancel_replace` / `cancel_replace_batch` (plus `_with_raw`), backed by `POST /orders/cancel-replace` and `/orders/cancel-replace/batch`. A single request cancels a resting order and submits its replacement atomically; the batch variant does so for many operations at once. Typed `CancelReplaceParams`, `CancelReplaceResult`, and `CancelReplaceBatchResponse`.
- Typed `orderEvent` payloads via `WebSocketClient::on_order_event_typed`, deserializing the socket.io `orderEvent` frame into an `OrderEvent` enum tagged on `type`:
  - `OrderEvent::Matched(MatchedOrderEvent)` — pre-settlement per-fill estimate (`source: "SETTLEMENT"`, `type: "MATCHED"`); monetary fields are JSON strings, `configuredFeeRateBps`/`effectiveFeeBps` are JSON numbers (maker side reports `0`), with `token` (`YES`/`NO`) and `isEstimate`.
  - `OrderEvent::Execution(ExecutionOrderEvent)` — FAK/FOK terminal outcome (`source: "OME"`, `type: "EXECUTION"`); `status` is `FILLED`/`PARTIALLY_FILLED`/`KILLED`, `eventId` is the string `"terminal:<orderId>"`.
  - `OrderEvent::Unknown` absorbs lifecycle frames (`PLACEMENT`/`UPDATE`/`CANCELLATION`/`MINED`/`FAILED`).
- The raw `WebSocketClient::on_order_event` handler is retained unchanged for callers that want the untyped `serde_json::Value`.
- Added the runnable `websocket_order_events` example covering the typed `orderEvent` subscription flow.
- Modeled the `POST /orders` `execution` response object on `OrderResponse` via the new optional `execution: Option<OrderExecution>` field, exposing the taker-delay outcome (`settlementStatus`, `eligibleAt`), settlement linkage (`tradeEventId`, `txHash`, `clientOrderId`), fees (`feeRateBps`, `effectiveFeeBps`), and the raw integer-string totals (`totalsRaw`). `settlementStatus` stays a plain string for forward-compatibility with server-added values. Additive and non-breaking.

### Changed

- `ExecutionOrderEvent::price` and `ExecutionOrderEvent::remaining_size` use `FlexFloat` to accept the JSON numbers OME emits (the static type previously documented for these fields was `string`, which never matched the wire value — the runtime value was always a JSON number). This is a no-op at runtime; only the static type is corrected.
- README, examples README, Cargo manifest, and lockfile now target `v1.1.0`.

### Fixed

- `PriceOracleMetadata.logo` is now `Option<String>` (with `#[serde(default)]`). Some markets omit this cosmetic field, and the previously-required typing failed deserialization of the entire `Market` in `markets.get_market()` / `get_active_markets()`. Brings Rust in line with the Go/TS SDKs, which already tolerated it.
- `OrderBook.last_trade_price` is now `Option<f64>` (with `#[serde(default)]`). The API returns `lastTradePrice: null` for markets with no trades yet, which previously failed deserialization of the whole book in `markets.get_order_book()`. Verified against the API source that only `lastTradePrice` is nullable — the other orderbook fields remain required.

## [1.0.13]

### Added

- Optional receive-window controls for normal and delegated order creation:
  - `ReceiveWindowOptions::timestamp`
  - `ReceiveWindowOptions::recv_window`, serialized as top-level `recvWindow`
- Opt-in receive-window methods:
  - `OrderClient::create_order_with_receive_window`
  - `DelegatedOrderService::create_order_with_receive_window`
- Unit coverage for omitted defaults, top-level-only payloads, automatic timestamp stamping, and invalid receive-window values before network calls.

### Changed

- Existing `create_order` method signatures are unchanged, preserving downstream struct-literal compatibility.
- `OrderClient` profile initialization now uses `PortfolioFetcher::get_current_profile` (`GET /profiles/me`) instead of address-based profile lookup.
- This release branch is based on the merged `v1.0.11` profile/me, partner account listing, and supported websocket channel cleanup changes.
- README, examples README, Cargo manifest, and lockfile now target `v1.0.13`.

## [1.0.11]

### Added

- Added `PortfolioFetcher::get_current_profile` for `GET /profiles/me`, fetching the authenticated caller's private profile without passing an address.
- Added `PartnerAccountService::list_accounts` for `GET /profiles/partner-accounts`, including typed list params/response models, optional address recovery, and `limit` capped at 25.
- Added public partner account list models:
  - `ListPartnerAccountsParams`
  - `PartnerAccountListItem`
  - `ListPartnerAccountsResponse`
- Added unit coverage for `/profiles/me` profile reads and HMAC-only partner account listing, filtering, pagination capping, and invalid query params.

### Changed

- Removed unsupported legacy websocket short channel variants and stale typed handlers/types for unsupported events.
- README, examples README, Cargo manifest, and lockfile now target `v1.0.11`.

## [1.0.10]

### Added

- Added partner withdrawal-address allowlist helpers:
  - `PartnerAccountService::add_withdrawal_address`
  - `PartnerAccountService::delete_withdrawal_address`
- Added typed withdrawal-address request/response models:
  - `PartnerWithdrawalAddressInput`
  - `PartnerWithdrawalAddressResponse`
- Added `HttpClient::delete_with_identity` and `RetryableClient::delete_with_identity` for identity-token authenticated DELETE requests.
- Added unit coverage for withdrawal payload serialization modes and withdrawal-address models/validation.
- Added WebSocket subscription/event surface for order events, live sports/esports, market lifecycle, oracle price data, and system messages.

### Changed

- `WithdrawServerWalletParams::on_behalf_of` is now optional so callers can submit authenticated caller wallet withdrawals to explicit allowed destinations.
- `WithdrawServerWalletParams` now omits unset optional fields from the JSON body.
- Server-wallet withdraw docs now describe omitted-destination smart-wallet fallback and explicit whitelisted treasury destinations.
- The server-wallet redeem/withdraw example can optionally allowlist a withdraw destination before submitting the HMAC withdraw request.
- README, examples README, Cargo manifest, and lockfile now target `v1.0.10`.

## [1.0.9] - 2026-04-30

### Added

- Added partner server-wallet allowance recovery endpoints:
  - `PartnerAccountService::check_allowances`
  - `PartnerAccountService::retry_allowances`
- Added typed allowance recovery response models for summaries, targets, and statuses.
- Added runnable `partner_account_allowances` example for partner HMAC allowance check and retry operations without admin APIs.

### Changed

- Updated partner allowance recovery models and docs for live-chain retry behavior:
  - target `submitted` status now means the current retry request submitted a sponsored transaction or user operation
  - target-level `IN_FLIGHT_ELSEWHERE`, `RATE_LIMITED`, and `nextRetryAt` modeling was removed
  - success response `retryAfterSeconds` / `nextRetryAt` modeling was removed; `429` retry timing remains available from the raw API error body
  - retry `429` and `409` responses are surfaced as `LimitlessError::Api` with the HTTP status and raw response body
- README, examples README, Cargo manifest, and lockfile now target `v1.0.9`.

## [1.0.8]

### Changed

- Updated websocket examples to use the current market-price subscription flow.

### Fixed

- Fixed the websocket transport handshake against the production Socket.IO endpoint and pinned the TLS crypto provider explicitly for Rustls.

## [1.0.7]

### Changed

- Migrated portfolio history endpoint from legacy page/limit pagination to cursor-based pagination.
  - `get_user_history()` now accepts `cursor: Option<&str>` and `limit: Option<u32>`.
  - First request should pass `None` for cursor; the SDK sends `cursor=` empty, and subsequent requests pass the returned `next_cursor`.
  - Default limit changed from 10 to 20 to match API default.
- Updated `HistoryEntry` struct to match current API response shape (`block_timestamp`, `strategy`, `transaction_hash`, `market`, etc.).
- Replaced `HistoryResponse.total_count` with `next_cursor: Option<String>` for cursor-based pagination.
- Added `HistoryMarket` and `HistoryMarketCollateral` structs.
- Expanded the example catalog with market-pages, user-orders, API-token revoke, and custom-client flows.

### Fixed

- Made `OrderMatch.created_at` optional to handle API responses that omit the field.
- Made `LatestTrade` price fields (`latest_yes_price`, `latest_no_price`, `outcome_token_price`) optional for markets without trades.

## [1.0.6]

### Added

- Initial crate foundation with shared HTTP transport, typed errors, logging, retry helpers, and HMAC signing.
- Root `Client` plus read-side parity for markets, portfolio, and market-pages APIs.
- Partner service parity for api tokens, partner accounts, and server-wallet redeem/withdraw flows.
- Order parity including args/types, validation, order builder, EIP-712 signer, and authenticated order client flows.
- Delegated-order parity for create/cancel/cancel-all flows on behalf of server-managed child profiles.
- WebSocket parity pass with socket.io transport, subscription helpers, typed event payloads, and reconnect handling.
- Added the full example catalog under `examples/`, mirroring the Go SDK examples and Cargo run targets.
