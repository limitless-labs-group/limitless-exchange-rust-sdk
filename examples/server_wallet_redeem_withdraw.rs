mod support;

use std::time::{SystemTime, UNIX_EPOCH};

use limitless_exchange_rust_sdk::{
    Client, CreatePartnerAccountInput, DeriveApiTokenInput, HmacCredentials,
    RedeemServerWalletParams, WithdrawServerWalletParams, SCOPE_ACCOUNT_CREATION,
    SCOPE_DELEGATED_SIGNING, SCOPE_TRADING, SCOPE_WITHDRAWAL,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let identity_token = support::require_env("LIMITLESS_IDENTITY_TOKEN");
    let market_slug = support::require_env("MARKET_SLUG");
    let skip_withdraw = support::env_flag("LIMITLESS_SKIP_WITHDRAW", true);
    let bootstrap = support::public_client()?;
    let unix = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();

    let requested_scopes = vec![
        SCOPE_TRADING.to_string(),
        SCOPE_DELEGATED_SIGNING.to_string(),
        SCOPE_ACCOUNT_CREATION.to_string(),
        SCOPE_WITHDRAWAL.to_string(),
    ];

    let capabilities = bootstrap
        .api_tokens
        .get_capabilities(&identity_token)
        .await?;
    println!(
        "Capabilities: enabled={} allowedScopes={:?}",
        capabilities.token_management_enabled, capabilities.allowed_scopes
    );

    let derived = bootstrap
        .api_tokens
        .derive_token(
            &identity_token,
            &DeriveApiTokenInput {
                label: Some(format!("rust-sdk-server-wallet-{unix}")),
                scopes: requested_scopes,
            },
        )
        .await?;
    println!(
        "Derived token: tokenId={} profileId={} scopes={:?}",
        derived.token_id, derived.profile.id, derived.scopes
    );

    let scoped = Client::from_http_client(
        Client::builder()
            .hmac_credentials(HmacCredentials {
                token_id: derived.token_id.clone(),
                secret: derived.secret.clone(),
            })
            .logger(support::logger())
            .build()?,
    )?;

    let market = bootstrap.markets.get_market(&market_slug).await?;
    let condition_id = market
        .condition_id
        .clone()
        .filter(|value| !value.trim().is_empty())
        .expect("market does not expose conditionId");

    let (on_behalf_of, account, created_account) =
        if let Some(existing_profile) = support::optional_positive_i32("LIMITLESS_ON_BEHALF_OF") {
            let account = support::optional_env("LIMITLESS_SERVER_WALLET_ACCOUNT")
                .unwrap_or_else(|| "(not provided)".to_string());
            println!("Using existing server-wallet child account from env.");
            (existing_profile, account, false)
        } else {
            let partner_account = scoped
                .partner_accounts
                .create_account(
                    &CreatePartnerAccountInput {
                        display_name: Some(support::optional_env_with_fallback(
                            "PARTNER_ACCOUNT_DISPLAY_NAME",
                            "Rust SDK Server Wallet",
                        )),
                        create_server_wallet: Some(true),
                    },
                    None,
                )
                .await?;
            (partner_account.profile_id, partner_account.account, true)
        };

    println!(
        "Server-wallet target: onBehalfOf={} account={} conditionId={}",
        on_behalf_of, account, condition_id
    );

    if created_account {
        println!("Redeem is usually most useful on an existing traded child profile. Set LIMITLESS_ON_BEHALF_OF to reuse one.");
    }

    let redeem = scoped
        .server_wallets
        .redeem_positions(&RedeemServerWalletParams {
            condition_id: condition_id.clone(),
            on_behalf_of,
        })
        .await?;
    println!(
        "Redeem submitted: transactionId={} userOperationHash={} wallet={}",
        redeem.envelope.transaction_id,
        redeem.envelope.user_operation_hash,
        redeem.envelope.wallet_address,
    );

    if skip_withdraw {
        println!("Skipping withdraw because LIMITLESS_SKIP_WITHDRAW is enabled. Set LIMITLESS_SKIP_WITHDRAW=0 to run the withdraw step.");
        return Ok(());
    }

    let amount = support::require_env("LIMITLESS_WITHDRAW_AMOUNT");
    let destination = support::optional_env("LIMITLESS_WITHDRAW_DESTINATION");
    let token = support::optional_env("LIMITLESS_WITHDRAW_TOKEN");

    println!(
        "Withdrawing amount={} token={} destination={}",
        amount,
        support::empty_fallback(token.as_deref(), "(default token)"),
        support::empty_fallback(destination.as_deref(), "(authenticated account default)")
    );

    let withdraw = scoped
        .server_wallets
        .withdraw(&WithdrawServerWalletParams {
            amount,
            on_behalf_of,
            token,
            destination,
        })
        .await?;
    println!(
        "Withdraw submitted: transactionId={} userOperationHash={} destination={}",
        withdraw.envelope.transaction_id,
        withdraw.envelope.user_operation_hash,
        withdraw.destination
    );

    Ok(())
}
