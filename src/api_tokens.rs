use serde::{Deserialize, Serialize};

use crate::{
    errors::{LimitlessError, Result},
    http_client::HttpClient,
};

#[derive(Clone)]
pub struct ApiTokenService {
    client: HttpClient,
}

impl ApiTokenService {
    pub fn new(client: HttpClient) -> Self {
        Self { client }
    }

    pub async fn derive_token(
        &self,
        identity_token: &str,
        input: &DeriveApiTokenInput,
    ) -> Result<DeriveApiTokenResponse> {
        if identity_token.trim().is_empty() {
            return Err(LimitlessError::invalid_input(
                "identity token is required for derive_token",
            ));
        }
        self.client
            .post_with_identity("/auth/api-tokens/derive", identity_token, input)
            .await
    }

    pub async fn list_tokens(&self) -> Result<Vec<ApiToken>> {
        self.client.require_auth("list_tokens")?;
        self.client.get("/auth/api-tokens").await
    }

    pub async fn get_capabilities(&self, identity_token: &str) -> Result<PartnerCapabilities> {
        if identity_token.trim().is_empty() {
            return Err(LimitlessError::invalid_input(
                "identity token is required for get_capabilities",
            ));
        }
        self.client
            .get_with_identity("/auth/api-tokens/capabilities", identity_token)
            .await
    }

    pub async fn revoke_token(&self, token_id: &str) -> Result<String> {
        self.client.require_auth("revoke_token")?;
        let response: MessageResponse = self
            .client
            .delete(&format!(
                "/auth/api-tokens/{}",
                urlencoding::encode(token_id)
            ))
            .await?;
        Ok(response.message)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeriveApiTokenInput {
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub scopes: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApiTokenProfile {
    pub id: i32,
    pub account: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeriveApiTokenResponse {
    #[serde(rename = "apiKey")]
    pub api_key: String,
    pub secret: String,
    #[serde(rename = "tokenId")]
    pub token_id: String,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    pub scopes: Vec<String>,
    pub profile: ApiTokenProfile,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApiToken {
    #[serde(rename = "tokenId")]
    pub token_id: String,
    #[serde(default)]
    pub label: Option<String>,
    pub scopes: Vec<String>,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "lastUsedAt", default)]
    pub last_used_at: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PartnerCapabilities {
    #[serde(rename = "partnerProfileId")]
    pub partner_profile_id: i32,
    #[serde(rename = "tokenManagementEnabled")]
    pub token_management_enabled: bool,
    #[serde(rename = "allowedScopes")]
    pub allowed_scopes: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct MessageResponse {
    message: String,
}

pub const SCOPE_TRADING: &str = "trading";
pub const SCOPE_ACCOUNT_CREATION: &str = "account_creation";
pub const SCOPE_DELEGATED_SIGNING: &str = "delegated_signing";
pub const SCOPE_WITHDRAWAL: &str = "withdrawal";
