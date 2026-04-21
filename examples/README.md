# Rust SDK Examples

These examples mirror the Go SDK example catalog and are intended to be run with Cargo:

```bash
cargo run --example <name>
```

Available examples:

- `active_markets`
- `custom_client`
- `market_pages`
- `portfolio`
- `user_orders`
- `api_tokens`
- `api_token_revoke`
- `clob_gtc_order`
- `clob_fak_order`
- `clob_fok_order`
- `negrisk_order`
- `delegated_order`
- `delegated_fok_order`
- `e2e_fok_flow`
- `server_wallet_redeem_withdraw`
- `websocket_orderbook`
- `websocket_positions`

Common environment variables:

- `LIMITLESS_API_KEY` for legacy authenticated HTTP or websocket examples
- `LIMITLESS_API_TOKEN_ID` and `LIMITLESS_API_TOKEN_SECRET` for scoped HMAC examples
- `LIMITLESS_IDENTITY_TOKEN` for derive-token and partner bootstrap flows
- `PRIVATE_KEY` for signed order examples
- `MARKET_SLUG` to override the default market slug used by trade and websocket examples
- `MARKET_PAGE_PATH` to override the default market-page path used by the market-pages example
- `MARKET_PAGE_TICKER_FILTER` and `MARKET_PAGE_DURATION_FILTER` for optional market-page filtering
- `PROFILE_ADDRESS` for the portfolio example
- `LIMITLESS_BASE_URL` and `LIMITLESS_STRATEGY_HEADER` for the custom-client example

Partner / delegated examples:

- `LIMITLESS_PARTNER_PROFILE_ID`
- `LIMITLESS_TARGET_FEE_RATE_BPS`
- `PARTNER_NAME`
- `LIMITLESS_DELEGATED_ACCOUNT_READY_DELAY_MS`
- `LIMITLESS_PLACE_DELEGATED_ORDER`

Server-wallet example:

- `LIMITLESS_SKIP_WITHDRAW`
- `LIMITLESS_WITHDRAW_AMOUNT`
- `LIMITLESS_WITHDRAW_DESTINATION`
- `LIMITLESS_WITHDRAW_TOKEN`
- `LIMITLESS_ON_BEHALF_OF`
- `LIMITLESS_SERVER_WALLET_ACCOUNT`

Order-management examples:

- `LIMITLESS_CANCEL_ALL_ORDERS=1` to enable the destructive cancel-all step in `user_orders`

API-token revoke example:

- `LIMITLESS_REVOKE_TOKEN_ID` to revoke a specific token in `api_token_revoke`

Notes:

- Public read examples do not require authentication.
- Trading, delegated, and server-wallet examples are subject to the geographic restrictions described in the repository README.
- Examples are reference integrations. Review them carefully before using them in production with real funds.
- Never hardcode `PRIVATE_KEY`, API tokens, or partner secrets in source files. Provide them through environment variables or your secret manager.
- `websocket_positions` accepts either `LIMITLESS_API_KEY` or scoped HMAC credentials.
- `server_wallet_redeem_withdraw` is only for child profiles created with `create_server_wallet = true`.
- `cargo check --examples` passes in this repository as of the current `1.0.7` release prep.
