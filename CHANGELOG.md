# Changelog

All notable changes to the Limitless Exchange Rust SDK will be documented in this file.

## [1.1.0]

### Added

- Typed `orderEvent` payloads via `WebSocketClient::on_order_event_typed`, deserializing the socket.io `orderEvent` frame into an `OrderEvent` enum tagged on `type`:
  - `OrderEvent::Matched(MatchedOrderEvent)` — pre-settlement per-fill estimate (`source: "SETTLEMENT"`, `type: "MATCHED"`); monetary fields are JSON strings, `configuredFeeRateBps`/`effectiveFeeBps` are JSON numbers (maker side reports `0`), with `token` (`YES`/`NO`) and `isEstimate`.
  - `OrderEvent::Execution(ExecutionOrderEvent)` — FAK/FOK terminal outcome (`source: "OME"`, `type: "EXECUTION"`); `status` is `FILLED`/`PARTIALLY_FILLED`/`KILLED`, `eventId` is the string `"terminal:<orderId>"`.
  - `OrderEvent::Unknown` absorbs lifecycle frames (`PLACEMENT`/`UPDATE`/`CANCELLATION`/`MINED`/`FAILED`).
- The raw `WebSocketClient::on_order_event` handler is retained unchanged for callers that want the untyped `serde_json::Value`.
- Added the runnable `websocket_order_events` example covering the typed `orderEvent` subscription flow.

### Changed

- `ExecutionOrderEvent::price` and `ExecutionOrderEvent::remaining_size` use `FlexFloat` to accept the JSON numbers OME emits (the static type previously documented for these fields was `string`, which never matched the wire value — the runtime value was always a JSON number). This is a no-op at runtime; only the static type is corrected.
- README, examples README, Cargo manifest, and lockfile now target `v1.1.0`.

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
