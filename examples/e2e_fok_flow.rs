mod support;

use std::time::{SystemTime, UNIX_EPOCH};

use limitless_exchange_rust_sdk::{
    CreateDelegatedOrderParams, CreatePartnerAccountInput, DeriveApiTokenInput, FokOrderArgs,
    OrderArgs, OrderType, Side, SCOPE_ACCOUNT_CREATION, SCOPE_DELEGATED_SIGNING, SCOPE_TRADING,
};
use tokio::time::{sleep, Duration};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let identity_token = support::require_env("LIMITLESS_IDENTITY_TOKEN");
    let market_slug = support::require_env("MARKET_SLUG");
    let partner_name = support::optional_env_with_fallback("PARTNER_NAME", "partner");
    let bootstrap = support::public_client()?;

    let requested_scopes = vec![
        SCOPE_TRADING.to_string(),
        SCOPE_DELEGATED_SIGNING.to_string(),
        SCOPE_ACCOUNT_CREATION.to_string(),
    ];

    println!("1. Read current partner capabilities with the Privy identity token.");
    let capabilities = bootstrap
        .api_tokens
        .get_capabilities(&identity_token)
        .await?;
    println!(
        "   Capabilities: enabled={} allowedScopes={:?}",
        capabilities.token_management_enabled, capabilities.allowed_scopes
    );

    println!("2. Derive a scoped HMAC token for partner operations.");
    let unix = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let derived = bootstrap
        .api_tokens
        .derive_token(
            &identity_token,
            &DeriveApiTokenInput {
                label: Some(format!("rust-sdk-e2e-fok-flow-{unix}")),
                scopes: requested_scopes,
            },
        )
        .await?;
    println!(
        "   Derived token: tokenId={} profileId={} scopes={:?}",
        derived.token_id, derived.profile.id, derived.scopes
    );

    let scoped_client = limitless_exchange_rust_sdk::Client::from_http_client(
        limitless_exchange_rust_sdk::Client::builder()
            .hmac_credentials(limitless_exchange_rust_sdk::HmacCredentials {
                token_id: derived.token_id.clone(),
                secret: derived.secret.clone(),
            })
            .logger(support::logger())
            .build()?,
    )?;

    println!("3. Verify the derived HMAC token works on authenticated partner endpoints.");
    let active_tokens = scoped_client.api_tokens.list_tokens().await?;
    println!(
        "   Active tokens visible to scoped client: {}",
        active_tokens.len()
    );

    println!("4. Fetch the market that will be used for delegated trading.");
    let market = bootstrap.markets.get_market(&market_slug).await?;
    let tokens = support::required_market_tokens(&market)?;
    let venue = market.venue.clone().expect("market has no venue");
    println!(
        "   Market: slug={} exchange={} collateral={}({})",
        market.slug,
        venue.exchange,
        market.collateral_token.symbol,
        market.collateral_token.address
    );

    println!("5. Create a partner-owned child account with a server wallet.");
    let partner_account = scoped_client
        .partner_accounts
        .create_account(
            &CreatePartnerAccountInput {
                display_name: Some(format!("{partner_name}-e2e-fok-{unix}")),
                create_server_wallet: Some(true),
            },
            None,
        )
        .await?;
    println!(
        "   Created partner account: profileId={} account={}",
        partner_account.profile_id, partner_account.account
    );

    println!("6. Important: fund the created account before attempting to trade.");
    println!(
        "   Fund {} with {} on {}.",
        partner_account.account, market.collateral_token.symbol, market.collateral_token.address,
    );
    println!("   Check delegated allowances with partner_accounts.check_allowances; retry missing or failed targets with retry_allowances, then poll again.");

    let ready_delay_ms =
        support::optional_non_negative_u64("LIMITLESS_DELEGATED_ACCOUNT_READY_DELAY_MS", 10_000);
    if ready_delay_ms > 0 {
        println!(
            "   Waiting {}ms before the delegated FOK trade step...",
            ready_delay_ms
        );
        sleep(Duration::from_millis(ready_delay_ms)).await;
    }

    if !support::env_flag("LIMITLESS_PLACE_DELEGATED_ORDER", false) {
        println!("7. Trading step skipped.");
        println!(
            "   Re-run with LIMITLESS_PLACE_DELEGATED_ORDER=1 after funding the created account."
        );
        return Ok(());
    }

    println!("7. Place a delegated FOK order with the HMAC-scoped client.");
    println!(
        "   Delegated FOK context: onBehalfOf={} account={} exchange={} collateral={}({})",
        partner_account.profile_id,
        partner_account.account,
        venue.exchange,
        market.collateral_token.symbol,
        market.collateral_token.address
    );

    let maker_amount = 1.0;
    println!("   FOK makerAmount={:.2} USDC side=BUY", maker_amount);

    let order = scoped_client
        .delegated_orders
        .create_order(CreateDelegatedOrderParams {
            market_slug,
            order_type: OrderType::Fok,
            on_behalf_of: partner_account.profile_id,
            fee_rate_bps: 300,
            args: OrderArgs::from(FokOrderArgs {
                token_id: tokens.yes,
                side: Side::Buy,
                maker_amount,
                expiration: None,
                nonce: None,
                taker: None,
            }),
        })
        .await?;
    println!("   Created delegated FOK order: orderId={}", order.order.id);

    println!("8. No cleanup step for this flow.");
    if !order.maker_matches.is_empty() {
        println!(
            "   Delegated FOK order matched immediately with {} fill(s).",
            order.maker_matches.len()
        );
    } else {
        println!("   Delegated FOK order was not matched and auto-cancelled by FOK semantics.");
    }

    Ok(())
}
