# Changelog

All notable changes to the Limitless Exchange Rust SDK will be documented in this file.

## [Unreleased]

### Added

- `PortfolioFetcher::get_current_profile` — fetches the authenticated caller's own private profile via `GET /profiles/me`, resolving the account from the request credentials so no address is required. Brings the Rust SDK to parity with the Go (`GetCurrentProfile`) and Python (`get_current_profile`) SDKs.

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
