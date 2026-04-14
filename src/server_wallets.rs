use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::{
    errors::{LimitlessError, Result},
    http_client::HttpClient,
};

static CONDITION_ID_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^0x[a-fA-F0-9]{64}$").expect("valid condition id regex"));
static INTEGER_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[0-9]+$").expect("valid integer regex"));
static ADDRESS_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^0x[a-fA-F0-9]{40}$").expect("valid address regex"));

const HMAC_ONLY_ERROR: &str =
    "Server wallet redeem/withdraw require HMAC-scoped API token auth; legacy API keys are not supported.";

#[derive(Clone)]
pub struct ServerWalletService {
    client: HttpClient,
}

impl ServerWalletService {
    pub fn new(client: HttpClient) -> Self {
        Self { client }
    }

    pub async fn redeem_positions(
        &self,
        params: &RedeemServerWalletParams,
    ) -> Result<RedeemServerWalletResponse> {
        self.require_hmac_auth("redeem_server_wallet_positions")?;
        validate_condition_id(&params.condition_id)?;
        validate_on_behalf_of(params.on_behalf_of)?;
        self.client.post("/portfolio/redeem", params).await
    }

    pub async fn withdraw(
        &self,
        params: &WithdrawServerWalletParams,
    ) -> Result<WithdrawServerWalletResponse> {
        self.require_hmac_auth("withdraw_server_wallet_funds")?;
        validate_amount(&params.amount)?;
        validate_on_behalf_of(params.on_behalf_of)?;

        if let Some(token) = &params.token {
            validate_address("token", token)?;
        }
        if let Some(destination) = &params.destination {
            validate_address("destination", destination)?;
        }

        self.client.post("/portfolio/withdraw", params).await
    }

    fn require_hmac_auth(&self, operation: &str) -> Result<()> {
        self.client.require_auth(operation)?;
        if self.client.hmac_credentials().is_none() {
            return Err(LimitlessError::invalid_input(HMAC_ONLY_ERROR));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RedeemServerWalletParams {
    #[serde(rename = "conditionId")]
    pub condition_id: String,
    #[serde(rename = "onBehalfOf")]
    pub on_behalf_of: i32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WithdrawServerWalletParams {
    pub amount: String,
    #[serde(rename = "onBehalfOf")]
    pub on_behalf_of: i32,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub destination: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ServerWalletTransactionEnvelope {
    pub hash: String,
    #[serde(rename = "userOperationHash")]
    pub user_operation_hash: String,
    #[serde(rename = "transactionId")]
    pub transaction_id: String,
    #[serde(rename = "walletAddress")]
    pub wallet_address: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RedeemServerWalletResponse {
    #[serde(flatten)]
    pub envelope: ServerWalletTransactionEnvelope,
    #[serde(rename = "conditionId")]
    pub condition_id: String,
    #[serde(rename = "marketId")]
    pub market_id: i32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WithdrawServerWalletResponse {
    #[serde(flatten)]
    pub envelope: ServerWalletTransactionEnvelope,
    pub token: String,
    pub destination: String,
    pub amount: String,
}

fn validate_condition_id(condition_id: &str) -> Result<()> {
    if CONDITION_ID_REGEX.is_match(condition_id) {
        return Ok(());
    }
    Err(LimitlessError::invalid_input(
        "condition_id must be a 0x-prefixed 32-byte hex string",
    ))
}

fn validate_on_behalf_of(on_behalf_of: i32) -> Result<()> {
    if on_behalf_of > 0 {
        return Ok(());
    }
    Err(LimitlessError::invalid_input(
        "on_behalf_of must be a positive integer",
    ))
}

fn validate_amount(amount: &str) -> Result<()> {
    if INTEGER_REGEX.is_match(amount) && amount != "0" {
        return Ok(());
    }
    Err(LimitlessError::invalid_input(
        "amount must be a positive integer string in the token smallest unit",
    ))
}

fn validate_address(field_name: &str, value: &str) -> Result<()> {
    if ADDRESS_REGEX.is_match(value) {
        return Ok(());
    }
    Err(LimitlessError::invalid_input(format!(
        "{field_name} must be a valid EVM address"
    )))
}

#[cfg(test)]
mod tests {
    use super::{validate_amount, validate_condition_id};

    #[test]
    fn validates_server_wallet_condition_id() {
        assert!(validate_condition_id(
            "0x9da2c66a47d1ae8b278a1474e2f449223f28eda420d4f130eab96ea6566f5d3f"
        )
        .is_ok());
        assert!(validate_condition_id("bad").is_err());
    }

    #[test]
    fn validates_server_wallet_amount() {
        assert!(validate_amount("1").is_ok());
        assert!(validate_amount("0").is_err());
        assert!(validate_amount("1.5").is_err());
    }
}
