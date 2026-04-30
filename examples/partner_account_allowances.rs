mod support;

use limitless_exchange_rust_sdk::{
    LimitlessError, PartnerAccountAllowanceResponse, PartnerAccountAllowanceTarget,
    PARTNER_ACCOUNT_ALLOWANCE_STATUS_FAILED, PARTNER_ACCOUNT_ALLOWANCE_STATUS_MISSING,
    PARTNER_ACCOUNT_ALLOWANCE_STATUS_SUBMITTED,
};
use serde_json::Value;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sdk = support::hmac_client()?;
    let profile_id = partner_account_profile_id();
    let skip_retry = support::env_flag("LIMITLESS_SKIP_ALLOWANCE_RETRY", false);

    println!("GET /profiles/partner-accounts/{profile_id}/allowances");
    let allowances = sdk.partner_accounts.check_allowances(profile_id).await?;
    print_allowance_response(&allowances);

    if allowances.ready {
        println!("Allowance targets are ready.");
        return Ok(());
    }
    if !has_retryable_missing_or_failed_target(&allowances.targets) {
        println!("No retryable missing or failed targets were returned.");
        return Ok(());
    }
    if skip_retry {
        println!("Skipping retry because LIMITLESS_SKIP_ALLOWANCE_RETRY is enabled.");
        return Ok(());
    }

    println!("POST /profiles/partner-accounts/{profile_id}/allowances/retry");
    match sdk.partner_accounts.retry_allowances(profile_id).await {
        Ok(retried) => {
            print_allowance_response(&retried);
            if submitted_targets(&retried.targets) > 0 {
                println!(
                    "Retry submitted sponsored allowance work. Poll the GET endpoint again after a short delay."
                );
            }
        }
        Err(err) => handle_retry_error(err)?,
    }

    Ok(())
}

fn partner_account_profile_id() -> i32 {
    support::optional_positive_i32("LIMITLESS_PARTNER_ACCOUNT_PROFILE_ID")
        .or_else(|| support::optional_positive_i32("LIMITLESS_ON_BEHALF_OF"))
        .unwrap_or_else(|| {
            panic!("LIMITLESS_PARTNER_ACCOUNT_PROFILE_ID environment variable is required")
        })
}

fn has_retryable_missing_or_failed_target(targets: &[PartnerAccountAllowanceTarget]) -> bool {
    targets.iter().any(|target| {
        target.retryable
            && matches!(
                target.status.as_str(),
                PARTNER_ACCOUNT_ALLOWANCE_STATUS_MISSING | PARTNER_ACCOUNT_ALLOWANCE_STATUS_FAILED
            )
    })
}

fn submitted_targets(targets: &[PartnerAccountAllowanceTarget]) -> usize {
    targets
        .iter()
        .filter(|target| target.status == PARTNER_ACCOUNT_ALLOWANCE_STATUS_SUBMITTED)
        .count()
}

fn handle_retry_error(err: LimitlessError) -> Result<(), Box<dyn std::error::Error>> {
    match &err {
        LimitlessError::Api(api_err) if api_err.status == 429 => {
            println!(
                "Retry is rate limited. retryAfterSeconds={}",
                retry_after_seconds(&api_err.data)
            );
        }
        LimitlessError::Api(api_err) if api_err.status == 409 => {
            println!(
                "Another allowance retry is already running. Wait briefly and poll the GET endpoint again."
            );
        }
        _ => {}
    }

    Err(Box::new(err))
}

fn retry_after_seconds(data: &Value) -> String {
    data.get("retryAfterSeconds")
        .and_then(Value::as_i64)
        .map(|value| value.to_string())
        .unwrap_or_else(|| "(not provided)".to_string())
}

fn print_allowance_response(resp: &PartnerAccountAllowanceResponse) {
    println!(
        "profileId={} partnerProfileId={} chainId={} wallet={} ready={}",
        resp.profile_id, resp.partner_profile_id, resp.chain_id, resp.wallet_address, resp.ready
    );
    println!(
        "summary: total={} confirmed={} missing={} submitted={} failed={}",
        resp.summary.total,
        resp.summary.confirmed,
        resp.summary.missing,
        resp.summary.submitted,
        resp.summary.failed
    );

    for (index, target) in resp.targets.iter().enumerate() {
        print!(
            "target[{index}]: type={} label={} requiredFor={} status={} confirmed={} retryable={} spenderOrOperator={}",
            target.target_type,
            target.label,
            target.required_for,
            target.status,
            target.confirmed,
            target.retryable,
            target.spender_or_operator
        );
        if let Some(transaction_id) = &target.transaction_id {
            print!(" transactionId={transaction_id}");
        }
        if let Some(tx_hash) = &target.tx_hash {
            print!(" txHash={tx_hash}");
        }
        if let Some(user_operation_hash) = &target.user_operation_hash {
            print!(" userOperationHash={user_operation_hash}");
        }
        if let Some(error_code) = &target.error_code {
            print!(" errorCode={error_code}");
        }
        if let Some(error_message) = &target.error_message {
            print!(" errorMessage={error_message:?}");
        }
        println!();
    }
}
