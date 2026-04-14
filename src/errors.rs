use reqwest::StatusCode;
use serde_json::{json, Value};
use thiserror::Error;

pub type Result<T> = std::result::Result<T, LimitlessError>;

#[derive(Debug, Clone, Error)]
#[error("API error {status} {method} {url}: {message}")]
pub struct ApiError {
    pub status: u16,
    pub message: String,
    pub data: Value,
    pub url: String,
    pub method: String,
}

impl ApiError {
    pub fn is_auth_error(&self) -> bool {
        matches!(self.status, 401 | 403)
    }
}

#[derive(Debug, Error)]
pub enum LimitlessError {
    #[error(transparent)]
    Api(#[from] ApiError),

    #[error("authentication is required for {operation}; pass an API key or HMAC credentials when creating the client")]
    AuthenticationRequired { operation: String },

    #[error("{0}")]
    InvalidInput(String),

    #[error("request failed: {0}")]
    Request(#[from] reqwest::Error),

    #[error("failed to decode response: {0}")]
    Decode(#[from] serde_json::Error),

    #[error("failed to decode HMAC secret: {0}")]
    Base64(#[from] base64::DecodeError),

    #[error("failed to sign request: {0}")]
    Signing(String),

    #[error("websocket error: {0}")]
    WebSocket(String),
}

impl LimitlessError {
    pub fn invalid_input(message: impl Into<String>) -> Self {
        Self::InvalidInput(message.into())
    }
}

pub fn parse_api_error(status: StatusCode, body: &[u8], url: &str, method: &str) -> LimitlessError {
    let fallback = format!("Request failed with status {}", status.as_u16());
    let parsed = serde_json::from_slice::<Value>(body)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(body).trim().to_string()));

    LimitlessError::Api(ApiError {
        status: status.as_u16(),
        message: extract_error_message(&parsed, &fallback),
        data: parsed,
        url: url.to_string(),
        method: method.to_string(),
    })
}

fn extract_error_message(data: &Value, fallback: &str) -> String {
    if data.is_null() {
        return fallback.to_string();
    }

    if let Some(message) = data.get("message") {
        if let Some(text) = message.as_str() {
            return text.to_string();
        }
        if let Some(items) = message.as_array() {
            let parts: Vec<String> = items
                .iter()
                .filter_map(|item| item.as_object())
                .map(|obj| {
                    obj.iter()
                        .map(|(key, value)| format!("{key}: {}", value_to_string(value)))
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .filter(|s| !s.is_empty())
                .collect();
            if !parts.is_empty() {
                return parts.join(" | ");
            }
        }
    }

    for key in ["error", "msg"] {
        if let Some(text) = data.get(key).and_then(Value::as_str) {
            return text.to_string();
        }
    }

    if let Some(errors) = data.get("errors") {
        return json!(errors).to_string();
    }

    if let Some(text) = data.as_str() {
        return text.to_string();
    }

    data.to_string()
}

fn value_to_string(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| value.to_string())
}
