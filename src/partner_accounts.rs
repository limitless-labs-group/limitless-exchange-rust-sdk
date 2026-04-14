use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{
    errors::{LimitlessError, Result},
    http_client::HttpClient,
};

const PARTNER_ACCOUNT_DISPLAY_NAME_MAX_LENGTH: usize = 44;

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
