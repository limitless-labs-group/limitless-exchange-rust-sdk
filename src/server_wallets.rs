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

        if let Some(on_behalf_of) = params.on_behalf_of {
            validate_on_behalf_of(on_behalf_of)?;
        }

        if let Some(token) = &params.token {
            validate_address("token", token)?;
        }
        if let Some(destination) = &params.destination {
            validate_address("destination", destination)?;
        }
        if params.on_behalf_of.is_none() && params.destination.is_none() {
            return Err(LimitlessError::invalid_input(
                "on_behalf_of or destination is required for withdraw",
            ));
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
    #[serde(
        rename = "onBehalfOf",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub on_behalf_of: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
    use serde_json::json;

    use crate::{hmac::HmacCredentials, http_client::HttpClient};

    use super::{
        validate_amount, validate_condition_id, ServerWalletService, WithdrawServerWalletParams,
    };

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

    #[test]
    fn serializes_withdraw_payload_modes() {
        assert_eq!(
            serde_json::to_value(&WithdrawServerWalletParams {
                amount: "1000000".to_string(),
                on_behalf_of: Some(12345),
                token: None,
                destination: None,
            })
            .unwrap(),
            json!({
                "amount": "1000000",
                "onBehalfOf": 12345
            })
        );

        assert_eq!(
            serde_json::to_value(&WithdrawServerWalletParams {
                amount: "1000000".to_string(),
                on_behalf_of: None,
                token: None,
                destination: Some("0x0F3262730c909408042F9Da345a916dc0e1F9787".to_string()),
            })
            .unwrap(),
            json!({
                "amount": "1000000",
                "destination": "0x0F3262730c909408042F9Da345a916dc0e1F9787"
            })
        );

        assert_eq!(
            serde_json::to_value(&WithdrawServerWalletParams {
                amount: "1000000".to_string(),
                on_behalf_of: Some(12345),
                token: None,
                destination: Some("0x0F3262730c909408042F9Da345a916dc0e1F9787".to_string()),
            })
            .unwrap(),
            json!({
                "amount": "1000000",
                "onBehalfOf": 12345,
                "destination": "0x0F3262730c909408042F9Da345a916dc0e1F9787"
            })
        );
    }

    #[tokio::test]
    async fn withdraw_validates_inputs_before_network() {
        let client = HttpClient::builder()
            .hmac_credentials(HmacCredentials {
                token_id: "token-1".to_string(),
                secret: "MDEyMzQ1Njc4OTAxMjM0NTY3ODkwMTIzNDU2Nzg5MDE=".to_string(),
            })
            .build()
            .unwrap();
        let service = ServerWalletService::new(client);

        let err = service
            .withdraw(&WithdrawServerWalletParams {
                amount: "1000000".to_string(),
                on_behalf_of: Some(0),
                token: None,
                destination: None,
            })
            .await
            .unwrap_err();
        assert_eq!(err.to_string(), "on_behalf_of must be a positive integer");

        let err = service
            .withdraw(&WithdrawServerWalletParams {
                amount: "1000000".to_string(),
                on_behalf_of: None,
                token: None,
                destination: None,
            })
            .await
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "on_behalf_of or destination is required for withdraw"
        );

        let err = service
            .withdraw(&WithdrawServerWalletParams {
                amount: "1000000".to_string(),
                on_behalf_of: Some(12345),
                token: None,
                destination: Some("not-an-address".to_string()),
            })
            .await
            .unwrap_err();
        assert_eq!(err.to_string(), "destination must be a valid EVM address");
    }
}
