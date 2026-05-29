use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::json;
use url::form_urlencoded::Serializer;

use crate::{
    errors::{LimitlessError, Result},
    http_client::HttpClient,
};

const PARTNER_ACCOUNT_DISPLAY_NAME_MAX_LENGTH: usize = 44;
const PARTNER_ACCOUNTS_MAX_LIMIT: u32 = 25;
const PARTNER_ACCOUNT_ALLOWANCE_HMAC_ONLY_ERROR: &str =
    "Partner account allowance recovery requires HMAC-scoped API token auth; legacy API keys are not supported.";
const PARTNER_ACCOUNT_LIST_HMAC_ONLY_ERROR: &str =
    "Partner account listing requires HMAC-scoped API token auth; legacy API keys are not supported.";

#[derive(Clone)]
pub struct PartnerAccountService {
    client: HttpClient,
}

impl PartnerAccountService {
    pub fn new(client: HttpClient) -> Self {
        Self { client }
    }

    pub async fn create_account(
        &self,
        input: &CreatePartnerAccountInput,
        eoa_headers: Option<&CreatePartnerAccountEoaHeaders>,
    ) -> Result<PartnerAccountResponse> {
        self.client.require_auth("create_partner_account")?;

        let server_wallet_mode = input.create_server_wallet.unwrap_or(false);
        if !server_wallet_mode && eoa_headers.is_none() {
            return Err(LimitlessError::invalid_input(
                "EOA headers are required when create_server_wallet is not true",
            ));
        }
        if let Some(display_name) = &input.display_name {
            if display_name.len() > PARTNER_ACCOUNT_DISPLAY_NAME_MAX_LENGTH {
                return Err(LimitlessError::invalid_input(format!(
                    "display_name must be at most {PARTNER_ACCOUNT_DISPLAY_NAME_MAX_LENGTH} characters"
                )));
            }
        }

        let mut headers = HashMap::new();
        if let Some(eoa_headers) = eoa_headers {
            headers.insert("x-account".to_string(), eoa_headers.account.clone());
            headers.insert(
                "x-signing-message".to_string(),
                eoa_headers.signing_message.clone(),
            );
            headers.insert("x-signature".to_string(), eoa_headers.signature.clone());
        }

        self.client
            .post_with_headers("/profiles/partner-accounts", input, headers)
            .await
    }

    /// Lists partner-owned accounts, or recovers a specific account by address when
    /// `params.account` is provided.
    pub async fn list_accounts(
        &self,
        params: &ListPartnerAccountsParams,
    ) -> Result<ListPartnerAccountsResponse> {
        self.require_hmac_auth(
            "list_partner_accounts",
            PARTNER_ACCOUNT_LIST_HMAC_ONLY_ERROR,
        )?;
        let path = partner_accounts_path(params)?;
        self.client.get(&path).await
    }

    /// Checks delegated-trading allowance readiness from live chain state for a partner-created
    /// server-wallet profile.
    pub async fn check_allowances(
        &self,
        profile_id: i32,
    ) -> Result<PartnerAccountAllowanceResponse> {
        self.require_allowance_hmac_auth("check_partner_account_allowances")?;
        let path = partner_account_allowances_path(profile_id)?;
        self.client.get(&path).await
    }

    /// Re-checks live chain state and retries delegated-trading allowances that are still missing
    /// for a partner-created server-wallet profile.
    ///
    /// Submitted targets in the response mean this retry request submitted a sponsored transaction
    /// or user operation; call `check_allowances` again after a short delay to observe confirmed
    /// chain state.
    pub async fn retry_allowances(
        &self,
        profile_id: i32,
    ) -> Result<PartnerAccountAllowanceResponse> {
        self.require_allowance_hmac_auth("retry_partner_account_allowances")?;
        let path = partner_account_allowances_path(profile_id)?;
        self.client.post(&format!("{path}/retry"), &json!({})).await
    }

    /// Adds an active partner withdrawal destination allowlist entry using a Privy identity token.
    /// API-token auth is not used for this endpoint.
    pub async fn add_withdrawal_address(
        &self,
        identity_token: &str,
        input: &PartnerWithdrawalAddressInput,
    ) -> Result<PartnerWithdrawalAddressResponse> {
        if identity_token.trim().is_empty() {
            return Err(LimitlessError::invalid_input(
                "identity token is required for add_withdrawal_address",
            ));
        }
        if input.address.trim().is_empty() {
            return Err(LimitlessError::invalid_input(
                "address is required for add_withdrawal_address",
            ));
        }

        self.client
            .post_with_identity("/portfolio/withdrawal-addresses", identity_token, input)
            .await
    }

    /// Removes a partner withdrawal destination allowlist entry using a Privy identity token.
    /// API-token auth is not used for this endpoint.
    pub async fn delete_withdrawal_address(
        &self,
        identity_token: &str,
        address: &str,
    ) -> Result<()> {
        if identity_token.trim().is_empty() {
            return Err(LimitlessError::invalid_input(
                "identity token is required for delete_withdrawal_address",
            ));
        }
        if address.trim().is_empty() {
            return Err(LimitlessError::invalid_input(
                "address is required for delete_withdrawal_address",
            ));
        }

        self.client
            .delete_with_identity(
                &format!(
                    "/portfolio/withdrawal-addresses/{}",
                    urlencoding::encode(address)
                ),
                identity_token,
            )
            .await
    }

    fn require_allowance_hmac_auth(&self, operation: &str) -> Result<()> {
        self.require_hmac_auth(operation, PARTNER_ACCOUNT_ALLOWANCE_HMAC_ONLY_ERROR)
    }

    fn require_hmac_auth(&self, operation: &str, error_message: &str) -> Result<()> {
        self.client.require_auth(operation)?;
        if self.client.hmac_credentials().is_none() {
            return Err(LimitlessError::invalid_input(error_message));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreatePartnerAccountInput {
    #[serde(rename = "displayName", default)]
    pub display_name: Option<String>,
    #[serde(rename = "createServerWallet", default)]
    pub create_server_wallet: Option<bool>,
}

#[derive(Clone, Debug)]
pub struct CreatePartnerAccountEoaHeaders {
    pub account: String,
    pub signing_message: String,
    pub signature: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PartnerAccountResponse {
    #[serde(rename = "profileId")]
    pub profile_id: i32,
    pub account: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ListPartnerAccountsParams {
    #[serde(default)]
    pub account: Option<String>,
    #[serde(default)]
    pub page: Option<u32>,
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PartnerAccountListItem {
    #[serde(rename = "profileId")]
    pub profile_id: i32,
    pub account: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ListPartnerAccountsResponse {
    pub data: Vec<PartnerAccountListItem>,
    pub page: u32,
    pub limit: u32,
    #[serde(rename = "hasMore")]
    pub has_more: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PartnerWithdrawalAddressInput {
    pub address: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PartnerWithdrawalAddressResponse {
    pub id: String,
    #[serde(rename = "profileId")]
    pub profile_id: i32,
    #[serde(rename = "destinationAddress")]
    pub destination_address: String,
    pub label: String,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "deletedAt")]
    pub deleted_at: Option<String>,
}

pub const PARTNER_ACCOUNT_ALLOWANCE_TYPE_USDC_ALLOWANCE: &str = "USDC_ALLOWANCE";
pub const PARTNER_ACCOUNT_ALLOWANCE_TYPE_CTF_APPROVAL: &str = "CTF_APPROVAL";

pub const PARTNER_ACCOUNT_ALLOWANCE_REQUIRED_FOR_BUY: &str = "BUY";
pub const PARTNER_ACCOUNT_ALLOWANCE_REQUIRED_FOR_SELL: &str = "SELL";

pub const PARTNER_ACCOUNT_ALLOWANCE_STATUS_CONFIRMED: &str = "confirmed";
pub const PARTNER_ACCOUNT_ALLOWANCE_STATUS_MISSING: &str = "missing";
pub const PARTNER_ACCOUNT_ALLOWANCE_STATUS_SUBMITTED: &str = "submitted";
pub const PARTNER_ACCOUNT_ALLOWANCE_STATUS_FAILED: &str = "failed";

pub const PARTNER_ACCOUNT_ALLOWANCE_ERROR_PRIVY_SPONSORSHIP_UNAVAILABLE: &str =
    "PRIVY_SPONSORSHIP_UNAVAILABLE";
pub const PARTNER_ACCOUNT_ALLOWANCE_ERROR_PRIVY_SUBMISSION_FAILED: &str = "PRIVY_SUBMISSION_FAILED";
pub const PARTNER_ACCOUNT_ALLOWANCE_ERROR_RPC_READ_FAILED: &str = "RPC_READ_FAILED";
pub const PARTNER_ACCOUNT_ALLOWANCE_ERROR_REQUEST_BUDGET_EXCEEDED: &str = "REQUEST_BUDGET_EXCEEDED";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PartnerAccountAllowanceSummary {
    pub total: i32,
    pub confirmed: i32,
    pub missing: i32,
    pub submitted: i32,
    pub failed: i32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PartnerAccountAllowanceTarget {
    #[serde(rename = "type")]
    pub target_type: String,
    #[serde(rename = "tokenAddress")]
    pub token_address: String,
    #[serde(rename = "spenderOrOperator")]
    pub spender_or_operator: String,
    pub label: String,
    #[serde(rename = "requiredFor")]
    pub required_for: String,
    pub confirmed: bool,
    pub status: String,
    #[serde(rename = "transactionId", default)]
    pub transaction_id: Option<String>,
    #[serde(rename = "txHash", default)]
    pub tx_hash: Option<String>,
    #[serde(rename = "userOperationHash", default)]
    pub user_operation_hash: Option<String>,
    pub retryable: bool,
    #[serde(rename = "errorCode", default)]
    pub error_code: Option<String>,
    #[serde(rename = "errorMessage", default)]
    pub error_message: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PartnerAccountAllowanceResponse {
    #[serde(rename = "profileId")]
    pub profile_id: i32,
    #[serde(rename = "partnerProfileId")]
    pub partner_profile_id: i32,
    #[serde(rename = "chainId")]
    pub chain_id: i32,
    #[serde(rename = "walletAddress")]
    pub wallet_address: String,
    pub ready: bool,
    pub summary: PartnerAccountAllowanceSummary,
    pub targets: Vec<PartnerAccountAllowanceTarget>,
}

fn partner_accounts_path(params: &ListPartnerAccountsParams) -> Result<String> {
    let mut query = Serializer::new(String::new());

    if let Some(account) = &params.account {
        let account = account.trim();
        if account.is_empty() {
            return Err(LimitlessError::invalid_input(
                "account must be a non-empty string",
            ));
        }
        query.append_pair("account", account);
    }

    if let Some(limit) = params.limit {
        if limit == 0 {
            return Err(LimitlessError::invalid_input(
                "limit must be a positive integer",
            ));
        }
        query.append_pair("limit", &limit.min(PARTNER_ACCOUNTS_MAX_LIMIT).to_string());
    }

    if let Some(page) = params.page {
        if page == 0 {
            return Err(LimitlessError::invalid_input(
                "page must be a positive integer",
            ));
        }
        query.append_pair("page", &page.to_string());
    }

    let encoded = query.finish();
    if encoded.is_empty() {
        Ok("/profiles/partner-accounts".to_string())
    } else {
        Ok(format!("/profiles/partner-accounts?{encoded}"))
    }
}

fn partner_account_allowances_path(profile_id: i32) -> Result<String> {
    if profile_id <= 0 {
        return Err(LimitlessError::invalid_input(
            "profile_id must be a positive integer",
        ));
    }
    Ok(format!(
        "/profiles/partner-accounts/{profile_id}/allowances"
    ))
}

#[cfg(test)]
mod tests {
    use reqwest::StatusCode;

    use crate::{
        errors::{parse_api_error, LimitlessError},
        hmac::HmacCredentials,
        http_client::HttpClient,
    };

    use super::{
        partner_account_allowances_path, partner_accounts_path, ListPartnerAccountsParams,
        ListPartnerAccountsResponse, PartnerAccountAllowanceResponse, PartnerAccountService,
        PartnerWithdrawalAddressInput, PartnerWithdrawalAddressResponse,
        PARTNER_ACCOUNT_ALLOWANCE_HMAC_ONLY_ERROR, PARTNER_ACCOUNT_LIST_HMAC_ONLY_ERROR,
    };

    #[test]
    fn builds_partner_account_allowances_path() {
        assert_eq!(
            partner_account_allowances_path(12345).unwrap(),
            "/profiles/partner-accounts/12345/allowances"
        );
        assert!(partner_account_allowances_path(0).is_err());
    }

    #[test]
    fn builds_partner_accounts_path() {
        assert_eq!(
            partner_accounts_path(&ListPartnerAccountsParams::default()).unwrap(),
            "/profiles/partner-accounts"
        );

        assert_eq!(
            partner_accounts_path(&ListPartnerAccountsParams {
                account: Some(" 0x1676716Ef7F19B5C5d690631CB57cf0bFD900A3d ".to_string()),
                limit: Some(100),
                page: Some(2),
            })
            .unwrap(),
            "/profiles/partner-accounts?account=0x1676716Ef7F19B5C5d690631CB57cf0bFD900A3d&limit=25&page=2"
        );
    }

    #[test]
    fn partner_accounts_path_rejects_invalid_params() {
        assert_eq!(
            partner_accounts_path(&ListPartnerAccountsParams {
                account: Some(" ".to_string()),
                ..Default::default()
            })
            .unwrap_err()
            .to_string(),
            "account must be a non-empty string"
        );

        assert_eq!(
            partner_accounts_path(&ListPartnerAccountsParams {
                limit: Some(0),
                ..Default::default()
            })
            .unwrap_err()
            .to_string(),
            "limit must be a positive integer"
        );

        assert_eq!(
            partner_accounts_path(&ListPartnerAccountsParams {
                page: Some(0),
                ..Default::default()
            })
            .unwrap_err()
            .to_string(),
            "page must be a positive integer"
        );
    }

    #[tokio::test]
    async fn list_accounts_rejects_legacy_api_key_only_auth() {
        let client = HttpClient::builder().api_key("api-key").build().unwrap();
        let service = PartnerAccountService::new(client);

        let err = service
            .list_accounts(&ListPartnerAccountsParams::default())
            .await
            .unwrap_err();
        assert_eq!(err.to_string(), PARTNER_ACCOUNT_LIST_HMAC_ONLY_ERROR);
    }

    #[tokio::test]
    async fn allowance_methods_reject_legacy_api_key_only_auth() {
        let client = HttpClient::builder().api_key("api-key").build().unwrap();
        let service = PartnerAccountService::new(client);

        let err = service.check_allowances(12345).await.unwrap_err();
        assert_eq!(err.to_string(), PARTNER_ACCOUNT_ALLOWANCE_HMAC_ONLY_ERROR);

        let err = service.retry_allowances(12345).await.unwrap_err();
        assert_eq!(err.to_string(), PARTNER_ACCOUNT_ALLOWANCE_HMAC_ONLY_ERROR);
    }

    #[tokio::test]
    async fn allowance_methods_validate_profile_id_before_network() {
        let client = HttpClient::builder()
            .hmac_credentials(HmacCredentials {
                token_id: "token-1".to_string(),
                secret: "MDEyMzQ1Njc4OTAxMjM0NTY3ODkwMTIzNDU2Nzg5MDE=".to_string(),
            })
            .build()
            .unwrap();
        let service = PartnerAccountService::new(client);

        let err = service.check_allowances(0).await.unwrap_err();
        assert_eq!(err.to_string(), "profile_id must be a positive integer");

        let err = service.retry_allowances(-1).await.unwrap_err();
        assert_eq!(err.to_string(), "profile_id must be a positive integer");
    }

    #[test]
    fn deserializes_partner_account_allowance_response() {
        let payload = serde_json::json!({
            "profileId": 12345,
            "partnerProfileId": 999,
            "chainId": 8453,
            "walletAddress": "0x1111111111111111111111111111111111111111",
            "ready": false,
            "summary": {
                "total": 1,
                "confirmed": 0,
                "missing": 0,
                "submitted": 1,
                "failed": 0
            },
            "targets": [{
                "type": "USDC_ALLOWANCE",
                "tokenAddress": "0x2222222222222222222222222222222222222222",
                "spenderOrOperator": "0x3333333333333333333333333333333333333333",
                "label": "ctf-exchange",
                "requiredFor": "BUY",
                "confirmed": false,
                "status": "submitted",
                "transactionId": "privy-transaction-id",
                "txHash": "0xabc",
                "userOperationHash": "0xdef",
                "retryable": false
            }]
        });

        let response: PartnerAccountAllowanceResponse = serde_json::from_value(payload).unwrap();
        assert_eq!(response.profile_id, 12345);
        assert_eq!(response.partner_profile_id, 999);
        assert_eq!(response.summary.submitted, 1);
        assert_eq!(response.targets.len(), 1);
        assert_eq!(response.targets[0].target_type, "USDC_ALLOWANCE");
        assert_eq!(
            response.targets[0].transaction_id.as_deref(),
            Some("privy-transaction-id")
        );
    }

    #[test]
    fn deserializes_partner_account_list_response() {
        let payload = serde_json::json!({
            "data": [{
                "profileId": 42,
                "account": "0x1676716Ef7F19B5C5d690631CB57cf0bFD900A3d",
                "displayName": "Partner User"
            }],
            "page": 1,
            "limit": 25,
            "hasMore": false
        });

        let response: ListPartnerAccountsResponse = serde_json::from_value(payload).unwrap();
        assert_eq!(response.page, 1);
        assert_eq!(response.limit, 25);
        assert!(!response.has_more);
        assert_eq!(response.data.len(), 1);
        assert_eq!(response.data[0].profile_id, 42);
        assert_eq!(response.data[0].display_name, "Partner User");
    }

    #[test]
    fn serializes_partner_withdrawal_address_input() {
        let payload = serde_json::to_value(&PartnerWithdrawalAddressInput {
            address: "0x0F3262730c909408042F9Da345a916dc0e1F9787".to_string(),
            label: Some("treasury".to_string()),
        })
        .unwrap();

        assert_eq!(
            payload,
            serde_json::json!({
                "address": "0x0F3262730c909408042F9Da345a916dc0e1F9787",
                "label": "treasury"
            })
        );
    }

    #[test]
    fn deserializes_partner_withdrawal_address_response() {
        let payload = serde_json::json!({
            "id": "11111111-1111-4111-8111-111111111111",
            "profileId": 1292711,
            "destinationAddress": "0x0F3262730c909408042F9Da345a916dc0e1F9787",
            "label": "treasury",
            "createdAt": "2026-04-30T12:00:00.000Z",
            "deletedAt": null
        });

        let response: PartnerWithdrawalAddressResponse = serde_json::from_value(payload).unwrap();
        assert_eq!(response.id, "11111111-1111-4111-8111-111111111111");
        assert_eq!(response.profile_id, 1292711);
        assert_eq!(
            response.destination_address,
            "0x0F3262730c909408042F9Da345a916dc0e1F9787"
        );
        assert_eq!(response.label, "treasury");
        assert_eq!(response.created_at, "2026-04-30T12:00:00.000Z");
        assert!(response.deleted_at.is_none());
    }

    #[tokio::test]
    async fn withdrawal_address_methods_validate_inputs_before_network() {
        let service = PartnerAccountService::new(HttpClient::builder().build().unwrap());

        let err = service
            .add_withdrawal_address(
                "",
                &PartnerWithdrawalAddressInput {
                    address: "0x0F3262730c909408042F9Da345a916dc0e1F9787".to_string(),
                    label: None,
                },
            )
            .await
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "identity token is required for add_withdrawal_address"
        );

        let err = service
            .add_withdrawal_address(
                "identity-token",
                &PartnerWithdrawalAddressInput {
                    address: String::new(),
                    label: None,
                },
            )
            .await
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "address is required for add_withdrawal_address"
        );

        let err = service
            .delete_withdrawal_address("", "0x0F3262730c909408042F9Da345a916dc0e1F9787")
            .await
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "identity token is required for delete_withdrawal_address"
        );

        let err = service
            .delete_withdrawal_address("identity-token", "")
            .await
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "address is required for delete_withdrawal_address"
        );
    }

    #[test]
    fn retry_error_responses_preserve_status_and_retry_body() {
        let rate_limited = parse_api_error(
            StatusCode::TOO_MANY_REQUESTS,
            br#"{"message":"rate limited","retryAfterSeconds":42}"#,
            "/profiles/partner-accounts/12345/allowances/retry",
            "POST",
        );

        match rate_limited {
            LimitlessError::Api(err) => {
                assert_eq!(err.status, 429);
                assert_eq!(
                    err.data
                        .get("retryAfterSeconds")
                        .and_then(serde_json::Value::as_i64),
                    Some(42)
                );
            }
            other => panic!("expected API error, got {other:?}"),
        }

        let conflict = parse_api_error(
            StatusCode::CONFLICT,
            br#"{"message":"allowance retry already running"}"#,
            "/profiles/partner-accounts/12345/allowances/retry",
            "POST",
        );

        match conflict {
            LimitlessError::Api(err) => assert_eq!(err.status, 409),
            other => panic!("expected API error, got {other:?}"),
        }
    }
}
