mod support;

use limitless_exchange_rust_sdk::{
    AmmAllowanceParams, AmmAllowanceResponse, AmmAllowanceSide, AmmBuyParams, LimitlessError,
};

/// Partner AMM trading flow using HMAC-scoped API token auth.
///
/// Required environment variables:
///   LIMITLESS_API_TOKEN_ID, LIMITLESS_API_TOKEN_SECRET  — HMAC credentials
///     (token must hold both `trading` and `delegated_signing` scopes)
///   LIMITLESS_AMM_MARKET                                — market slug or FPMM address
///   LIMITLESS_AMM_COLLATERAL_AMOUNT                     — positive integer base units
///   LIMITLESS_AMM_IDEMPOTENCY_KEY                       — required per trade
/// Optional:
///   LIMITLESS_ON_BEHALF_OF     — sub-account profile id (omit for direct profile)
///   LIMITLESS_AMM_OUTCOME      — 0 (YES, default) or 1 (NO)
///   LIMITLESS_AMM_SLIPPAGE_BPS — 0..1000 (defaults to server-side 100 when omitted)
///   LIMITLESS_AMM_SKIP_BUY     — when true, only run the allowance flow
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sdk = support::hmac_client()?;
    let market = support::require_env("LIMITLESS_AMM_MARKET");
    let on_behalf_of = support::optional_positive_i32("LIMITLESS_ON_BEHALF_OF");
    let outcome_index = support::optional_positive_i32("LIMITLESS_AMM_OUTCOME").unwrap_or(0);

    let allowance_params = AmmAllowanceParams {
        market: market.clone(),
        side: AmmAllowanceSide::Buy,
        on_behalf_of,
    };

    // 1. Ensure the BUY allowance is confirmed (check -> approve-once -> poll check).
    println!("POST /amm/allowances/check (BUY) for market {market}");
    let ensured = sdk.amm.ensure_allowance(&allowance_params, None).await?;
    print_allowance(&ensured);
    if !ensured.confirmed {
        println!("Allowance is not confirmed yet; re-run once the approval settles.");
        return Ok(());
    }

    if support::env_flag("LIMITLESS_AMM_SKIP_BUY", false) {
        println!("LIMITLESS_AMM_SKIP_BUY is set; skipping the buy submission.");
        return Ok(());
    }

    // 2. Submit an exact-collateral buy. Buy never re-checks allowances.
    let buy_params = AmmBuyParams {
        market: market.clone(),
        outcome_index,
        collateral_amount: support::require_env("LIMITLESS_AMM_COLLATERAL_AMOUNT"),
        slippage_bps: support::optional_positive_i32("LIMITLESS_AMM_SLIPPAGE_BPS"),
        idempotency_key: support::require_env("LIMITLESS_AMM_IDEMPOTENCY_KEY"),
        on_behalf_of,
    };

    println!("POST /amm/buy for market {market} (outcomeIndex={outcome_index})");
    // The `_with_raw` sibling exposes the HTTP status/headers alongside the decoded value.
    match sdk.amm.buy_with_raw(&buy_params).await {
        Ok(response) => {
            println!("HTTP {} — buy submitted", response.raw.status);
            let buy = response.data;
            println!(
                "status={:?} market={} collateralAmount={} expectedShares={} minShares={}",
                buy.status, buy.market, buy.collateral_amount, buy.expected_shares, buy.min_shares
            );
            if let Some(tx) = &buy.identifiers.transaction_id {
                println!("transactionId={tx}");
            }
            if let Some(op) = &buy.identifiers.user_operation_hash {
                println!("userOperationHash={op}");
            }
            if let Some(hash) = &buy.identifiers.tx_hash {
                println!("txHash={hash}");
            }
        }
        Err(err) => handle_trade_error(err)?,
    }

    Ok(())
}

fn print_allowance(resp: &AmmAllowanceResponse) {
    println!(
        "status={:?} confirmed={} side={:?} wallet={} spenderOrOperator={}",
        resp.status, resp.confirmed, resp.side, resp.wallet_address, resp.spender_or_operator
    );
    if let Some(current) = &resp.current_allowance {
        println!("currentAllowance={current}");
    }
}

fn handle_trade_error(err: LimitlessError) -> Result<(), Box<dyn std::error::Error>> {
    if let LimitlessError::Api(api) = &err {
        let hint = match api.status {
            403 => "auth rejected (legacy key, missing scopes, or unauthorized onBehalfOf)",
            409 => "market not tradable or idempotency conflict",
            422 => "insufficient balance / invalid quote / amount too small for slippage",
            425 => "trading temporarily blocked (maintenance / mode)",
            429 => "rate limited (10 requests / 10s across all AMM routes)",
            502 | 503 => "upstream temporarily unavailable; retry with the same idempotency key",
            _ => "AMM request failed",
        };
        println!("AMM error {} — {hint}", api.status);
    }
    Err(Box::new(err))
}
