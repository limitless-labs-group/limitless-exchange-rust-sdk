use serde::{Deserialize, Serialize};

use crate::{
    errors::{LimitlessError, Result},
    http_client::{HttpClient, RequestOptions},
    raw_response::SdkResponse,
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
        Ok(self
            .derive_token_with_raw(identity_token, input)
            .await?
            .data)
    }

    pub async fn derive_token_with_raw(
        &self,
        identity_token: &str,
        input: &DeriveApiTokenInput,
    ) -> Result<SdkResponse<DeriveApiTokenResponse>> {
        if identity_token.trim().is_empty() {
            return Err(LimitlessError::invalid_input(
                "identity token is required for derive_token",
            ));
        }
        let raw = self
            .client
            .post_raw_with_identity(
                "/auth/api-tokens/derive",
                identity_token,
                input,
                RequestOptions::default(),
            )
            .await?;
        let data = raw.json()?;
        Ok(SdkResponse { data, raw })
    }

    pub async fn list_tokens(&self) -> Result<Vec<ApiToken>> {
        Ok(self.list_tokens_with_raw().await?.data)
    }

    pub async fn list_tokens_with_raw(&self) -> Result<SdkResponse<Vec<ApiToken>>> {
        self.client.require_auth("list_tokens")?;
        let raw = self
            .client
            .get_raw("/auth/api-tokens", RequestOptions::default())
            .await?;
        let data = raw.json()?;
        Ok(SdkResponse { data, raw })
    }

    pub async fn get_capabilities(&self, identity_token: &str) -> Result<PartnerCapabilities> {
        Ok(self.get_capabilities_with_raw(identity_token).await?.data)
    }

    pub async fn get_capabilities_with_raw(
        &self,
        identity_token: &str,
    ) -> Result<SdkResponse<PartnerCapabilities>> {
        if identity_token.trim().is_empty() {
            return Err(LimitlessError::invalid_input(
                "identity token is required for get_capabilities",
            ));
        }
        let raw = self
            .client
            .get_raw_with_identity(
                "/auth/api-tokens/capabilities",
                identity_token,
                RequestOptions::default(),
            )
            .await?;
        let data = raw.json()?;
        Ok(SdkResponse { data, raw })
    }

    pub async fn revoke_token(&self, token_id: &str) -> Result<String> {
        Ok(self.revoke_token_with_raw(token_id).await?.data)
    }

    pub async fn revoke_token_with_raw(&self, token_id: &str) -> Result<SdkResponse<String>> {
        self.client.require_auth("revoke_token")?;
        let raw = self
            .client
            .delete_raw(
                &format!("/auth/api-tokens/{}", urlencoding::encode(token_id)),
                RequestOptions::default(),
            )
            .await?;
        let message: MessageResponse = raw.json()?;
        Ok(SdkResponse {
            data: message.message,
            raw,
        })
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
