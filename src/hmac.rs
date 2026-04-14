use base64::Engine;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use zeroize::{Zeroize, Zeroizing};

use crate::errors::{LimitlessError, Result};

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HmacCredentials {
    pub token_id: String,
    pub secret: String,
}

impl Drop for HmacCredentials {
    fn drop(&mut self) {
        self.secret.zeroize();
    }
}

pub fn build_hmac_message(timestamp: &str, method: &str, path: &str, body: &str) -> String {
    format!(
        "{}\n{}\n{}\n{}",
        timestamp,
        method.to_uppercase(),
        path,
        body
    )
}

pub fn compute_hmac_signature(
    secret: &str,
    timestamp: &str,
    method: &str,
    path: &str,
    body: &str,
) -> Result<String> {
    let key = Zeroizing::new(base64::engine::general_purpose::STANDARD.decode(secret)?);
    let mut mac =
        HmacSha256::new_from_slice(&key).map_err(|err| LimitlessError::Signing(err.to_string()))?;
    mac.update(build_hmac_message(timestamp, method, path, body).as_bytes());
    Ok(base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes()))
}

#[cfg(test)]
mod tests {
    use super::build_hmac_message;

    #[test]
    fn builds_hmac_message() {
        let message = build_hmac_message("ts", "post", "/orders", r#"{"x":1}"#);
        assert_eq!(message, "ts\nPOST\n/orders\n{\"x\":1}");
    }
}
