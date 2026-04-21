# Changelog

All notable changes to the Limitless Exchange Rust SDK will be documented in this file.

## [1.0.8]

### Changed

- Migrated portfolio history endpoint from legacy page/limit pagination to cursor-based pagination.
  - `get_user_history()` now accepts `cursor: Option<&str>` and `limit: Option<u32>`.
  - First request should pass `None` for cursor; the SDK sends `cursor=` empty, and subsequent requests pass the returned `next_cursor`.
  - Default limit changed from 10 to 20 to match API default.
- Updated `HistoryEntry` struct to match current API response shape (`block_timestamp`, `strategy`, `transaction_hash`, `market`, etc.).
- Replaced `HistoryResponse.total_count` with `next_cursor: Option<String>` for cursor-based pagination.
- Added `HistoryMarket` and `HistoryMarketCollateral` structs.
- Expanded the example catalog with market-pages, user-orders, API-token revoke, and custom-client flows.
- Updated websocket examples to use the current market-price subscription flow.

### Fixed

- Made `OrderMatch.created_at` optional to handle API responses that omit the field.
- Made `LatestTrade` price fields (`latest_yes_price`, `latest_no_price`, `outcome_token_price`) optional for markets without trades.
- Fixed the websocket transport handshake against the production Socket.IO endpoint and pinned the TLS crypto provider explicitly for Rustls.

## [1.0.6]

### Added

- Initial crate foundation with shared HTTP transport, typed errors, logging, retry helpers, and HMAC signing.
- Root `Client` plus read-side parity for markets, portfolio, and market-pages APIs.
- Partner service parity for api tokens, partner accounts, and server-wallet redeem/withdraw flows.
- Order parity including args/types, validation, order builder, EIP-712 signer, and authenticated order client flows.
- Delegated-order parity for create/cancel/cancel-all flows on behalf of server-managed child profiles.
- WebSocket parity pass with socket.io transport, subscription helpers, typed event payloads, and reconnect handling.
- Added the full example catalog under `examples/`, mirroring the Go SDK examples and Cargo run targets.
