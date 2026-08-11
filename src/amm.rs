use std::time::Duration;

use num_bigint::BigUint;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::{
    errors::{LimitlessError, Result},
    http_client::{HttpClient, RawResponse, RequestOptions},
    raw_response::SdkResponse,
};

/// Error returned when AMM operations are attempted with legacy-API-key-only auth.
pub const AMM_HMAC_ONLY_ERROR: &str =
    "AMM operations require HMAC-scoped API token auth or an explicit Privy identity token; legacy API keys are not supported.";

/// Default interval between allowance-confirmation polls in [`AmmService::ensure_allowance`].
pub const AMM_DEFAULT_ALLOWANCE_POLL_INTERVAL: Duration = Duration::from_secs(2);
/// Default maximum number of allowance-confirmation polls in [`AmmService::ensure_allowance`].
pub const AMM_DEFAULT_ALLOWANCE_MAX_ATTEMPTS: usize = 30;

const AMM_MARKET_MAX_LENGTH: usize = 255;
const AMM_AMOUNT_MAX_LENGTH: usize = 78;
const AMM_IDEMPOTENCY_KEY_MAX_LENGTH: usize = 128;
const AMM_MAX_ON_BEHALF_OF: i32 = 2_147_483_647;
const AMM_MIN_SLIPPAGE_BPS: i32 = 0;
const AMM_MAX_SLIPPAGE_BPS: i32 = 1000;

const AMM_ALLOWANCE_CHECK_PATH: &str = "/amm/allowances/check";
const AMM_ALLOWANCE_APPROVE_PATH: &str = "/amm/allowances/approve";
const AMM_BUY_PATH: &str = "/amm/buy";
const AMM_SELL_PATH: &str = "/amm/sell";

static AMM_POSITIVE_INTEGER_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[1-9][0-9]*$").expect("valid positive-integer regex"));
static AMM_MAX_UINT256: Lazy<BigUint> =
    Lazy::new(|| (BigUint::from(1_u8) << 256_u32) - BigUint::from(1_u8));

/// Identifies which market approval is being checked or submitted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AmmAllowanceSide {
    /// Collateral spending approval for the market FPMM (buys).
    #[serde(rename = "BUY")]
    Buy,
    /// Conditional Tokens operator approval for the market FPMM (sells).
    #[serde(rename = "SELL")]
    Sell,
}

/// Current state of an AMM approval (lowercase on the wire).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AmmAllowanceStatus {
    #[serde(rename = "missing")]
    Missing,
    #[serde(rename = "submitted")]
    Submitted,
    #[serde(rename = "confirmed")]
    Confirmed,
}

/// Submission state returned by an AMM trade (uppercase on the wire).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AmmTradeStatus {
    #[serde(rename = "SUBMITTED")]
    Submitted,
}

/// Selects the market and side for an allowance check or approve.
///
/// `market` may be a market slug or a checksummed FPMM address. Leave
/// `on_behalf_of` as `None` when the authenticated profile directly owns the
/// server wallet; set it to a sub-account profile id otherwise.
#[derive(Clone, Debug)]
pub struct AmmAllowanceParams {
    pub market: String,
    pub side: AmmAllowanceSide,
    pub on_behalf_of: Option<i32>,
}

/// An exact-collateral AMM buy request.
///
/// `collateral_amount` must be a positive integer string in the collateral
/// token's base units. `slippage_bps` `None` uses the API default; `idempotency_key`
/// is required and retained by the API for 24 hours.
#[derive(Clone, Debug)]
pub struct AmmBuyParams {
    pub market: String,
    pub outcome_index: i32,
    pub collateral_amount: String,
    pub slippage_bps: Option<i32>,
    pub idempotency_key: String,
    pub on_behalf_of: Option<i32>,
}

/// An exact-collateral-return AMM sell request.
///
/// `collateral_return_amount` must be a positive integer string in the
/// collateral token's base units. `slippage_bps` `None` uses the API default;
/// `idempotency_key` is required and retained by the API for 24 hours.
#[derive(Clone, Debug)]
pub struct AmmSellParams {
    pub market: String,
    pub outcome_index: i32,
    pub collateral_return_amount: String,
    pub slippage_bps: Option<i32>,
    pub idempotency_key: String,
    pub on_behalf_of: Option<i32>,
}

/// Configures [`AmmService::ensure_allowance`] polling.
#[derive(Clone, Debug)]
pub struct AmmAllowancePollOptions {
    /// Interval between allowance-confirmation polls.
    pub interval: Duration,
    /// Maximum number of confirmation polls before returning the last response.
    pub max_attempts: usize,
}

impl Default for AmmAllowancePollOptions {
    fn default() -> Self {
        Self {
            interval: AMM_DEFAULT_ALLOWANCE_POLL_INTERVAL,
            max_attempts: AMM_DEFAULT_ALLOWANCE_MAX_ATTEMPTS,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct AmmAllowanceRequest {
    market: String,
    side: AmmAllowanceSide,
    #[serde(
        rename = "onBehalfOf",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    on_behalf_of: Option<i32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct AmmBuyRequest {
    market: String,
    #[serde(rename = "outcomeIndex")]
    outcome_index: i32,
    #[serde(rename = "collateralAmount")]
    collateral_amount: String,
    #[serde(
        rename = "slippageBps",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    slippage_bps: Option<i32>,
    #[serde(rename = "idempotencyKey")]
    idempotency_key: String,
    #[serde(
        rename = "onBehalfOf",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    on_behalf_of: Option<i32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct AmmSellRequest {
    market: String,
    #[serde(rename = "outcomeIndex")]
    outcome_index: i32,
    #[serde(rename = "collateralReturnAmount")]
    collateral_return_amount: String,
    #[serde(
        rename = "slippageBps",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    slippage_bps: Option<i32>,
    #[serde(rename = "idempotencyKey")]
    idempotency_key: String,
    #[serde(
        rename = "onBehalfOf",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    on_behalf_of: Option<i32>,
}

/// Independently-optional transaction identifiers returned by sponsored
/// server-wallet operations. Any subset may be present.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AmmTransactionIdentifiers {
    #[serde(
        rename = "transactionId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub transaction_id: Option<String>,
    #[serde(
        rename = "userOperationHash",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub user_operation_hash: Option<String>,
    #[serde(rename = "txHash", default, skip_serializing_if = "Option::is_none")]
    pub tx_hash: Option<String>,
}

/// Returned by allowance check and approve operations. `current_allowance` is
/// present for BUY checks and omitted for SELL checks.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AmmAllowanceResponse {
    #[serde(flatten)]
    pub identifiers: AmmTransactionIdentifiers,
    pub status: AmmAllowanceStatus,
    pub confirmed: bool,
    pub market: String,
    #[serde(rename = "marketAddress")]
    pub market_address: String,
    pub side: AmmAllowanceSide,
    #[serde(rename = "walletAddress")]
    pub wallet_address: String,
    #[serde(rename = "tokenAddress")]
    pub token_address: String,
    #[serde(rename = "spenderOrOperator")]
    pub spender_or_operator: String,
    #[serde(
        rename = "currentAllowance",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub current_allowance: Option<String>,
}

/// Returned after an AMM buy has been submitted.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AmmBuyResponse {
    #[serde(flatten)]
    pub identifiers: AmmTransactionIdentifiers,
    pub status: AmmTradeStatus,
    pub market: String,
    #[serde(rename = "outcomeIndex")]
    pub outcome_index: i32,
    #[serde(rename = "collateralAmount")]
    pub collateral_amount: String,
    #[serde(rename = "expectedShares")]
    pub expected_shares: String,
    #[serde(rename = "minShares")]
    pub min_shares: String,
}

/// Returned after an AMM sell has been submitted.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AmmSellResponse {
    #[serde(flatten)]
    pub identifiers: AmmTransactionIdentifiers,
    pub status: AmmTradeStatus,
    pub market: String,
    #[serde(rename = "outcomeIndex")]
    pub outcome_index: i32,
    #[serde(rename = "collateralReturnAmount")]
    pub collateral_return_amount: String,
    #[serde(rename = "expectedShares")]
    pub expected_shares: String,
    #[serde(rename = "maxShares")]
    pub max_shares: String,
}

/// Manages AMM market approvals and server-wallet buy/sell submissions.
///
/// All four endpoints require HMAC-scoped API token auth (both `trading` and
/// `delegated_signing` scopes) or an explicit Privy identity token; legacy API
/// keys are rejected. Use the `*_with_identity` variants to pass a Privy token.
#[derive(Clone)]
pub struct AmmService {
    client: HttpClient,
}

impl AmmService {
    pub fn new(client: HttpClient) -> Self {
        Self { client }
    }

    // ----- check allowance -----

    /// Reads the live BUY or SELL approval state using configured HMAC auth.
    pub async fn check_allowance(
        &self,
        params: &AmmAllowanceParams,
    ) -> Result<AmmAllowanceResponse> {
        Ok(self.check_allowance_with_raw(params).await?.data)
    }

    /// Raw-response sibling of [`AmmService::check_allowance`].
    pub async fn check_allowance_with_raw(
        &self,
        params: &AmmAllowanceParams,
    ) -> Result<SdkResponse<AmmAllowanceResponse>> {
        self.require_hmac_auth("check_amm_allowance")?;
        self.allowance_call(AMM_ALLOWANCE_CHECK_PATH, params, None)
            .await
    }

    /// Reads the live BUY or SELL approval state using Privy identity auth.
    pub async fn check_allowance_with_identity(
        &self,
        identity_token: &str,
        params: &AmmAllowanceParams,
    ) -> Result<AmmAllowanceResponse> {
        Ok(self
            .check_allowance_with_identity_raw(identity_token, params)
            .await?
            .data)
    }

    /// Raw-response sibling of [`AmmService::check_allowance_with_identity`].
    pub async fn check_allowance_with_identity_raw(
        &self,
        identity_token: &str,
        params: &AmmAllowanceParams,
    ) -> Result<SdkResponse<AmmAllowanceResponse>> {
        let token = require_amm_identity_token(identity_token, "check_amm_allowance")?;
        self.allowance_call(AMM_ALLOWANCE_CHECK_PATH, params, Some(&token))
            .await
    }

    // ----- approve allowance -----

    /// Submits a missing BUY or SELL approval using configured HMAC auth.
    /// A submitted response is not confirmation; poll [`AmmService::check_allowance`]
    /// until `confirmed` is true.
    pub async fn approve_allowance(
        &self,
        params: &AmmAllowanceParams,
    ) -> Result<AmmAllowanceResponse> {
        Ok(self.approve_allowance_with_raw(params).await?.data)
    }

    /// Raw-response sibling of [`AmmService::approve_allowance`].
    pub async fn approve_allowance_with_raw(
        &self,
        params: &AmmAllowanceParams,
    ) -> Result<SdkResponse<AmmAllowanceResponse>> {
        self.require_hmac_auth("approve_amm_allowance")?;
        self.allowance_call(AMM_ALLOWANCE_APPROVE_PATH, params, None)
            .await
    }

    /// Submits a missing BUY or SELL approval using Privy identity auth.
    pub async fn approve_allowance_with_identity(
        &self,
        identity_token: &str,
        params: &AmmAllowanceParams,
    ) -> Result<AmmAllowanceResponse> {
        Ok(self
            .approve_allowance_with_identity_raw(identity_token, params)
            .await?
            .data)
    }

    /// Raw-response sibling of [`AmmService::approve_allowance_with_identity`].
    pub async fn approve_allowance_with_identity_raw(
        &self,
        identity_token: &str,
        params: &AmmAllowanceParams,
    ) -> Result<SdkResponse<AmmAllowanceResponse>> {
        let token = require_amm_identity_token(identity_token, "approve_amm_allowance")?;
        self.allowance_call(AMM_ALLOWANCE_APPROVE_PATH, params, Some(&token))
            .await
    }

    // ----- ensure allowance -----

    /// Checks an allowance, approves it at most once when missing, then polls
    /// allowance check until confirmation using configured HMAC auth. Buy and
    /// sell never call this workflow automatically.
    pub async fn ensure_allowance(
        &self,
        params: &AmmAllowanceParams,
        options: Option<AmmAllowancePollOptions>,
    ) -> Result<AmmAllowanceResponse> {
        Ok(self.ensure_allowance_with_raw(params, options).await?.data)
    }

    /// Raw-response sibling of [`AmmService::ensure_allowance`]; the raw response
    /// is that of the final allowance check (or approve) call.
    pub async fn ensure_allowance_with_raw(
        &self,
        params: &AmmAllowanceParams,
        options: Option<AmmAllowancePollOptions>,
    ) -> Result<SdkResponse<AmmAllowanceResponse>> {
        self.require_hmac_auth("ensure_amm_allowance")?;
        self.ensure_allowance_impl(params, None, options.unwrap_or_default())
            .await
    }

    /// Checks, approves-once, and polls an allowance using Privy identity auth.
    pub async fn ensure_allowance_with_identity(
        &self,
        identity_token: &str,
        params: &AmmAllowanceParams,
        options: Option<AmmAllowancePollOptions>,
    ) -> Result<AmmAllowanceResponse> {
        Ok(self
            .ensure_allowance_with_identity_raw(identity_token, params, options)
            .await?
            .data)
    }

    /// Raw-response sibling of [`AmmService::ensure_allowance_with_identity`].
    pub async fn ensure_allowance_with_identity_raw(
        &self,
        identity_token: &str,
        params: &AmmAllowanceParams,
        options: Option<AmmAllowancePollOptions>,
    ) -> Result<SdkResponse<AmmAllowanceResponse>> {
        let token = require_amm_identity_token(identity_token, "ensure_amm_allowance")?;
        self.ensure_allowance_impl(params, Some(&token), options.unwrap_or_default())
            .await
    }

    // ----- buy -----

    /// Submits an exact-collateral AMM buy using configured HMAC auth. It does
    /// not check or submit allowances. Reuse the same params when retrying so the
    /// serialized body and idempotency key remain unchanged.
    pub async fn buy(&self, params: &AmmBuyParams) -> Result<AmmBuyResponse> {
        Ok(self.buy_with_raw(params).await?.data)
    }

    /// Raw-response sibling of [`AmmService::buy`].
    pub async fn buy_with_raw(&self, params: &AmmBuyParams) -> Result<SdkResponse<AmmBuyResponse>> {
        self.require_hmac_auth("buy_amm_shares")?;
        self.buy_call(params, None).await
    }

    /// Submits an exact-collateral AMM buy using Privy identity auth.
    pub async fn buy_with_identity(
        &self,
        identity_token: &str,
        params: &AmmBuyParams,
    ) -> Result<AmmBuyResponse> {
        Ok(self
            .buy_with_identity_raw(identity_token, params)
            .await?
            .data)
    }

    /// Raw-response sibling of [`AmmService::buy_with_identity`].
    pub async fn buy_with_identity_raw(
        &self,
        identity_token: &str,
        params: &AmmBuyParams,
    ) -> Result<SdkResponse<AmmBuyResponse>> {
        let token = require_amm_identity_token(identity_token, "buy_amm_shares")?;
        self.buy_call(params, Some(&token)).await
    }

    // ----- sell -----

    /// Submits an exact-collateral-return AMM sell using configured HMAC auth. It
    /// does not check or submit allowances. Reuse the same params when retrying so
    /// the serialized body and idempotency key remain unchanged.
    pub async fn sell(&self, params: &AmmSellParams) -> Result<AmmSellResponse> {
        Ok(self.sell_with_raw(params).await?.data)
    }

    /// Raw-response sibling of [`AmmService::sell`].
    pub async fn sell_with_raw(
        &self,
        params: &AmmSellParams,
    ) -> Result<SdkResponse<AmmSellResponse>> {
        self.require_hmac_auth("sell_amm_shares")?;
        self.sell_call(params, None).await
    }

    /// Submits an exact-collateral-return AMM sell using Privy identity auth.
    pub async fn sell_with_identity(
        &self,
        identity_token: &str,
        params: &AmmSellParams,
    ) -> Result<AmmSellResponse> {
        Ok(self
            .sell_with_identity_raw(identity_token, params)
            .await?
            .data)
    }

    /// Raw-response sibling of [`AmmService::sell_with_identity`].
    pub async fn sell_with_identity_raw(
        &self,
        identity_token: &str,
        params: &AmmSellParams,
    ) -> Result<SdkResponse<AmmSellResponse>> {
        let token = require_amm_identity_token(identity_token, "sell_amm_shares")?;
        self.sell_call(params, Some(&token)).await
    }

    // ----- internal helpers -----

    async fn allowance_call(
        &self,
        path: &str,
        params: &AmmAllowanceParams,
        identity_token: Option<&str>,
    ) -> Result<SdkResponse<AmmAllowanceResponse>> {
        let request = build_allowance_request(params)?;
        let raw = self.post_amm(path, &request, identity_token).await?;
        let data = raw.json()?;
        Ok(SdkResponse { data, raw })
    }

    async fn buy_call(
        &self,
        params: &AmmBuyParams,
        identity_token: Option<&str>,
    ) -> Result<SdkResponse<AmmBuyResponse>> {
        let request = build_buy_request(params)?;
        let raw = self
            .post_amm(AMM_BUY_PATH, &request, identity_token)
            .await?;
        let data = raw.json()?;
        Ok(SdkResponse { data, raw })
    }

    async fn sell_call(
        &self,
        params: &AmmSellParams,
        identity_token: Option<&str>,
    ) -> Result<SdkResponse<AmmSellResponse>> {
        let request = build_sell_request(params)?;
        let raw = self
            .post_amm(AMM_SELL_PATH, &request, identity_token)
            .await?;
        let data = raw.json()?;
        Ok(SdkResponse { data, raw })
    }

    async fn ensure_allowance_impl(
        &self,
        params: &AmmAllowanceParams,
        identity_token: Option<&str>,
        options: AmmAllowancePollOptions,
    ) -> Result<SdkResponse<AmmAllowanceResponse>> {
        let checked = self
            .allowance_call(AMM_ALLOWANCE_CHECK_PATH, params, identity_token)
            .await?;
        if checked.data.confirmed {
            return Ok(checked);
        }

        let approved = self
            .allowance_call(AMM_ALLOWANCE_APPROVE_PATH, params, identity_token)
            .await?;
        if approved.data.confirmed {
            return Ok(approved);
        }

        let mut last = approved;
        for _ in 0..options.max_attempts {
            tokio::time::sleep(options.interval).await;
            let checked = self
                .allowance_call(AMM_ALLOWANCE_CHECK_PATH, params, identity_token)
                .await?;
            if checked.data.confirmed {
                return Ok(checked);
            }
            last = checked;
        }
        Ok(last)
    }

    async fn post_amm<B: Serialize + ?Sized>(
        &self,
        path: &str,
        body: &B,
        identity_token: Option<&str>,
    ) -> Result<RawResponse> {
        match identity_token {
            Some(token) => {
                self.client
                    .post_raw_with_identity(path, token, body, RequestOptions::default())
                    .await
            }
            None => {
                self.client
                    .post_raw(path, body, RequestOptions::default())
                    .await
            }
        }
    }

    fn require_hmac_auth(&self, operation: &str) -> Result<()> {
        self.client.require_auth(operation)?;
        if self.client.hmac_credentials().is_none() {
            return Err(LimitlessError::invalid_input(AMM_HMAC_ONLY_ERROR));
        }
        Ok(())
    }
}

fn require_amm_identity_token(identity_token: &str, operation: &str) -> Result<String> {
    let trimmed = identity_token.trim();
    if trimmed.is_empty() {
        return Err(LimitlessError::invalid_input(format!(
            "identity token is required for {operation}"
        )));
    }
    Ok(trimmed.to_string())
}

fn build_allowance_request(params: &AmmAllowanceParams) -> Result<AmmAllowanceRequest> {
    let market = validate_amm_market(&params.market)?;
    validate_amm_on_behalf_of(params.on_behalf_of)?;
    Ok(AmmAllowanceRequest {
        market,
        side: params.side,
        on_behalf_of: params.on_behalf_of,
    })
}

fn build_buy_request(params: &AmmBuyParams) -> Result<AmmBuyRequest> {
    let market = validate_amm_trade_common(
        &params.market,
        params.outcome_index,
        params.slippage_bps,
        &params.idempotency_key,
        params.on_behalf_of,
    )?;
    validate_amm_amount(&params.collateral_amount, "collateral_amount")?;
    Ok(AmmBuyRequest {
        market,
        outcome_index: params.outcome_index,
        collateral_amount: params.collateral_amount.clone(),
        slippage_bps: params.slippage_bps,
        idempotency_key: params.idempotency_key.clone(),
        on_behalf_of: params.on_behalf_of,
    })
}

fn build_sell_request(params: &AmmSellParams) -> Result<AmmSellRequest> {
    let market = validate_amm_trade_common(
        &params.market,
        params.outcome_index,
        params.slippage_bps,
        &params.idempotency_key,
        params.on_behalf_of,
    )?;
    validate_amm_amount(&params.collateral_return_amount, "collateral_return_amount")?;
    Ok(AmmSellRequest {
        market,
        outcome_index: params.outcome_index,
        collateral_return_amount: params.collateral_return_amount.clone(),
        slippage_bps: params.slippage_bps,
        idempotency_key: params.idempotency_key.clone(),
        on_behalf_of: params.on_behalf_of,
    })
}

fn validate_amm_trade_common(
    market: &str,
    outcome_index: i32,
    slippage_bps: Option<i32>,
    idempotency_key: &str,
    on_behalf_of: Option<i32>,
) -> Result<String> {
    let market = validate_amm_market(market)?;
    if outcome_index != 0 && outcome_index != 1 {
        return Err(LimitlessError::invalid_input(
            "outcome_index must be 0 (YES) or 1 (NO)",
        ));
    }
    if let Some(slippage_bps) = slippage_bps {
        if !(AMM_MIN_SLIPPAGE_BPS..=AMM_MAX_SLIPPAGE_BPS).contains(&slippage_bps) {
            return Err(LimitlessError::invalid_input(format!(
                "slippage_bps must be between {AMM_MIN_SLIPPAGE_BPS} and {AMM_MAX_SLIPPAGE_BPS}"
            )));
        }
    }
    if idempotency_key.trim().is_empty() {
        return Err(LimitlessError::invalid_input("idempotency_key is required"));
    }
    if idempotency_key.chars().count() > AMM_IDEMPOTENCY_KEY_MAX_LENGTH {
        return Err(LimitlessError::invalid_input(format!(
            "idempotency_key must be at most {AMM_IDEMPOTENCY_KEY_MAX_LENGTH} characters"
        )));
    }
    validate_amm_on_behalf_of(on_behalf_of)?;
    Ok(market)
}

fn validate_amm_market(market: &str) -> Result<String> {
    let market = market.trim();
    if market.is_empty() {
        return Err(LimitlessError::invalid_input("market is required"));
    }
    if market.chars().count() > AMM_MARKET_MAX_LENGTH {
        return Err(LimitlessError::invalid_input(format!(
            "market must be at most {AMM_MARKET_MAX_LENGTH} characters"
        )));
    }
    Ok(market.to_string())
}

fn validate_amm_amount(value: &str, field: &str) -> Result<()> {
    if value.len() > AMM_AMOUNT_MAX_LENGTH || !AMM_POSITIVE_INTEGER_REGEX.is_match(value) {
        return Err(amm_amount_error(field));
    }
    let parsed =
        BigUint::parse_bytes(value.as_bytes(), 10).ok_or_else(|| amm_amount_error(field))?;
    if parsed > *AMM_MAX_UINT256 {
        return Err(amm_amount_error(field));
    }
    Ok(())
}

fn amm_amount_error(field: &str) -> LimitlessError {
    LimitlessError::invalid_input(format!(
        "{field} must be a positive integer string in the collateral token base unit"
    ))
}

fn validate_amm_on_behalf_of(on_behalf_of: Option<i32>) -> Result<()> {
    if let Some(value) = on_behalf_of {
        if !(1..=AMM_MAX_ON_BEHALF_OF).contains(&value) {
            return Err(LimitlessError::invalid_input(format!(
                "on_behalf_of must be between 1 and {AMM_MAX_ON_BEHALF_OF}"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::hmac::HmacCredentials;

    fn hmac_client() -> HttpClient {
        HttpClient::builder()
            .hmac_credentials(HmacCredentials {
                token_id: "token-1".to_string(),
                secret: "MDEyMzQ1Njc4OTAxMjM0NTY3ODkwMTIzNDU2Nzg5MDE=".to_string(),
            })
            .build()
            .expect("client")
    }

    fn valid_buy_params() -> AmmBuyParams {
        AmmBuyParams {
            market: "btc-above-100k".to_string(),
            outcome_index: 0,
            collateral_amount: "1000000".to_string(),
            slippage_bps: Some(100),
            idempotency_key: "idem-key-1".to_string(),
            on_behalf_of: None,
        }
    }

    fn valid_sell_params() -> AmmSellParams {
        AmmSellParams {
            market: "btc-above-100k".to_string(),
            outcome_index: 1,
            collateral_return_amount: "500000".to_string(),
            slippage_bps: None,
            idempotency_key: "idem-key-2".to_string(),
            on_behalf_of: Some(326),
        }
    }

    #[test]
    fn allowance_request_buy_serializes_side_and_on_behalf_of() {
        let request = build_allowance_request(&AmmAllowanceParams {
            market: "  btc-above-100k  ".to_string(),
            side: AmmAllowanceSide::Buy,
            on_behalf_of: Some(326),
        })
        .expect("request should build");

        assert_eq!(
            serde_json::to_value(&request).unwrap(),
            json!({
                "market": "btc-above-100k",
                "side": "BUY",
                "onBehalfOf": 326
            })
        );
    }

    #[test]
    fn allowance_request_sell_omits_on_behalf_of_when_absent() {
        let request = build_allowance_request(&AmmAllowanceParams {
            market: "btc-above-100k".to_string(),
            side: AmmAllowanceSide::Sell,
            on_behalf_of: None,
        })
        .expect("request should build");

        assert_eq!(
            serde_json::to_value(&request).unwrap(),
            json!({
                "market": "btc-above-100k",
                "side": "SELL"
            })
        );
    }

    #[test]
    fn buy_request_serializes_expected_wire_shape() {
        let request = build_buy_request(&valid_buy_params()).expect("buy request should build");
        assert_eq!(
            serde_json::to_value(&request).unwrap(),
            json!({
                "market": "btc-above-100k",
                "outcomeIndex": 0,
                "collateralAmount": "1000000",
                "slippageBps": 100,
                "idempotencyKey": "idem-key-1"
            })
        );
    }

    #[test]
    fn sell_request_omits_optional_slippage_and_keeps_on_behalf_of() {
        let request = build_sell_request(&valid_sell_params()).expect("sell request should build");
        assert_eq!(
            serde_json::to_value(&request).unwrap(),
            json!({
                "market": "btc-above-100k",
                "outcomeIndex": 1,
                "collateralReturnAmount": "500000",
                "idempotencyKey": "idem-key-2",
                "onBehalfOf": 326
            })
        );
    }

    #[test]
    fn buy_request_body_is_byte_stable_across_serializations() {
        let params = valid_buy_params();
        let first = serde_json::to_string(&build_buy_request(&params).unwrap()).unwrap();
        let second = serde_json::to_string(&build_buy_request(&params).unwrap()).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn allowance_response_buy_parses_current_allowance() {
        let response: AmmAllowanceResponse = serde_json::from_value(json!({
            "status": "confirmed",
            "confirmed": true,
            "market": "btc-above-100k",
            "marketAddress": "0x1111111111111111111111111111111111111111",
            "side": "BUY",
            "walletAddress": "0x2222222222222222222222222222222222222222",
            "tokenAddress": "0x3333333333333333333333333333333333333333",
            "spenderOrOperator": "0x4444444444444444444444444444444444444444",
            "currentAllowance": "1000000000",
            "transactionId": "privy-tx-id"
        }))
        .expect("allowance response should deserialize");

        assert_eq!(response.status, AmmAllowanceStatus::Confirmed);
        assert!(response.confirmed);
        assert_eq!(response.side, AmmAllowanceSide::Buy);
        assert_eq!(response.current_allowance.as_deref(), Some("1000000000"));
        assert_eq!(
            response.identifiers.transaction_id.as_deref(),
            Some("privy-tx-id")
        );
        assert!(response.identifiers.tx_hash.is_none());
    }

    #[test]
    fn allowance_response_sell_omits_current_allowance() {
        let response: AmmAllowanceResponse = serde_json::from_value(json!({
            "status": "missing",
            "confirmed": false,
            "market": "btc-above-100k",
            "marketAddress": "0x1111111111111111111111111111111111111111",
            "side": "SELL",
            "walletAddress": "0x2222222222222222222222222222222222222222",
            "tokenAddress": "0x3333333333333333333333333333333333333333",
            "spenderOrOperator": "0x4444444444444444444444444444444444444444"
        }))
        .expect("allowance response should deserialize");

        assert_eq!(response.status, AmmAllowanceStatus::Missing);
        assert!(!response.confirmed);
        assert_eq!(response.side, AmmAllowanceSide::Sell);
        assert!(response.current_allowance.is_none());
    }

    #[test]
    fn buy_response_parses_submitted_status_and_optional_identifiers() {
        let response: AmmBuyResponse = serde_json::from_value(json!({
            "status": "SUBMITTED",
            "market": "btc-above-100k",
            "outcomeIndex": 0,
            "collateralAmount": "1000000",
            "expectedShares": "1900000",
            "minShares": "1880000",
            "userOperationHash": "0xuserop"
        }))
        .expect("buy response should deserialize");

        assert_eq!(response.status, AmmTradeStatus::Submitted);
        assert_eq!(response.min_shares, "1880000");
        assert_eq!(
            response.identifiers.user_operation_hash.as_deref(),
            Some("0xuserop")
        );
        assert!(response.identifiers.transaction_id.is_none());
        assert!(response.identifiers.tx_hash.is_none());
    }

    #[test]
    fn sell_response_round_trips_through_json() {
        let original = AmmSellResponse {
            identifiers: AmmTransactionIdentifiers {
                transaction_id: Some("tx-1".to_string()),
                user_operation_hash: None,
                tx_hash: Some("0xhash".to_string()),
            },
            status: AmmTradeStatus::Submitted,
            market: "btc-above-100k".to_string(),
            outcome_index: 1,
            collateral_return_amount: "500000".to_string(),
            expected_shares: "1000000".to_string(),
            max_shares: "1020000".to_string(),
        };
        let encoded = serde_json::to_value(&original).unwrap();
        let decoded: AmmSellResponse = serde_json::from_value(encoded).unwrap();
        assert_eq!(decoded.identifiers.transaction_id.as_deref(), Some("tx-1"));
        assert_eq!(decoded.identifiers.tx_hash.as_deref(), Some("0xhash"));
        assert!(decoded.identifiers.user_operation_hash.is_none());
        assert_eq!(decoded.max_shares, "1020000");
    }

    #[test]
    fn validates_amount_bounds_including_uint256_max() {
        // 2^256 - 1 is accepted, 2^256 is rejected.
        let max = (BigUint::from(1_u8) << 256_u32) - BigUint::from(1_u8);
        assert!(validate_amm_amount(&max.to_string(), "amount").is_ok());

        let over = BigUint::from(1_u8) << 256_u32;
        assert!(validate_amm_amount(&over.to_string(), "amount").is_err());

        assert!(validate_amm_amount("0", "amount").is_err());
        assert!(validate_amm_amount("01", "amount").is_err());
        assert!(validate_amm_amount("1.5", "amount").is_err());
        assert!(validate_amm_amount("1e6", "amount").is_err());
        assert!(validate_amm_amount(&"9".repeat(79), "amount").is_err());
    }

    #[tokio::test]
    async fn all_operations_reject_legacy_api_key_only_auth() {
        let client = HttpClient::builder().api_key("legacy-key").build().unwrap();
        let service = AmmService::new(client);
        let allowance = AmmAllowanceParams {
            market: "btc-above-100k".to_string(),
            side: AmmAllowanceSide::Buy,
            on_behalf_of: None,
        };

        for err in [
            service.check_allowance(&allowance).await.unwrap_err(),
            service.approve_allowance(&allowance).await.unwrap_err(),
            service.buy(&valid_buy_params()).await.unwrap_err(),
            service.sell(&valid_sell_params()).await.unwrap_err(),
            service
                .ensure_allowance(&allowance, None)
                .await
                .unwrap_err(),
        ] {
            assert_eq!(err.to_string(), AMM_HMAC_ONLY_ERROR);
        }
    }

    #[tokio::test]
    async fn buy_validates_params_before_network() {
        let service = AmmService::new(hmac_client());

        let mut bad_outcome = valid_buy_params();
        bad_outcome.outcome_index = 2;
        let err = service.buy(&bad_outcome).await.unwrap_err();
        assert!(matches!(err, LimitlessError::InvalidInput(_)));
        assert!(err.to_string().contains("outcome_index"));

        let mut bad_amount = valid_buy_params();
        bad_amount.collateral_amount = "0".to_string();
        assert!(matches!(
            service.buy(&bad_amount).await.unwrap_err(),
            LimitlessError::InvalidInput(_)
        ));

        let mut bad_slippage = valid_buy_params();
        bad_slippage.slippage_bps = Some(1001);
        assert!(matches!(
            service.buy(&bad_slippage).await.unwrap_err(),
            LimitlessError::InvalidInput(_)
        ));

        let mut blank_key = valid_buy_params();
        blank_key.idempotency_key = "   ".to_string();
        assert!(matches!(
            service.buy(&blank_key).await.unwrap_err(),
            LimitlessError::InvalidInput(_)
        ));

        let mut bad_on_behalf = valid_buy_params();
        bad_on_behalf.on_behalf_of = Some(0);
        assert!(matches!(
            service.buy(&bad_on_behalf).await.unwrap_err(),
            LimitlessError::InvalidInput(_)
        ));
    }

    #[tokio::test]
    async fn allowance_validates_market_before_network() {
        let service = AmmService::new(hmac_client());
        let err = service
            .check_allowance(&AmmAllowanceParams {
                market: "   ".to_string(),
                side: AmmAllowanceSide::Buy,
                on_behalf_of: None,
            })
            .await
            .unwrap_err();
        assert_eq!(err.to_string(), "market is required");
    }

    #[tokio::test]
    async fn identity_variants_require_non_blank_token() {
        let service = AmmService::new(hmac_client());
        let err = service
            .buy_with_identity("   ", &valid_buy_params())
            .await
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "identity token is required for buy_amm_shares"
        );
    }
}
