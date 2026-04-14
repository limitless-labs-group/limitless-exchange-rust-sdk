# Rust SDK Examples

These examples mirror the Go SDK example catalog and are intended to be run with Cargo:

```bash
cargo run --example <name>
```

Available examples:

- `active_markets`
- `portfolio`
- `api_tokens`
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
- `PROFILE_ADDRESS` for the portfolio example

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

Notes:

- Public read examples do not require authentication.
- `websocket_positions` accepts either `LIMITLESS_API_KEY` or scoped HMAC credentials.
- `server_wallet_redeem_withdraw` is only for child profiles created with `create_server_wallet = true`.
- This environment does not currently have `cargo`/`rustc`, so example compilation was not verified here.
