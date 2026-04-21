use std::{
    collections::{HashMap, HashSet},
    env, fmt,
    sync::{Arc, RwLock},
    time::Duration,
};

use reqwest::{header::HeaderMap, Method};
use serde::{de::DeserializeOwned, Serialize};
use url::Url;

use crate::{
    constants::DEFAULT_API_URL,
    errors::{parse_api_error, LimitlessError, Result},
    hmac::{compute_hmac_signature, HmacCredentials},
    logger::{noop_logger, SharedLogger},
    time_utils::chrono_timestamp,
};

const SDK_ID: &str = "lmts-sdk-rs";

#[derive(Clone)]
pub struct HttpClient {
    inner: Arc<HttpClientInner>,
}

struct HttpClientInner {
    client: reqwest::Client,
    state: RwLock<HttpClientState>,
}

struct HttpClientState {
    base_url: String,
    api_key: Option<String>,
    hmac_credentials: Option<HmacCredentials>,
    headers: HashMap<String, String>,
    logger: SharedLogger,
}

#[derive(Clone, Debug, Default)]
pub struct RequestOptions {
    allowed_statuses: HashSet<u16>,
}

impl RequestOptions {
    #[must_use]
    pub fn allow_status(mut self, code: u16) -> Self {
        self.allowed_statuses.insert(code);
        self
    }
}

#[derive(Clone, Debug)]
pub struct RawResponse {
    pub status: u16,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
}

impl RawResponse {
    pub fn json<T: DeserializeOwned>(&self) -> Result<T> {
        Ok(serde_json::from_slice(&self.body)?)
    }
}

#[derive(Default)]
pub struct HttpClientBuilder {
    base_url: Option<String>,
    timeout: Option<Duration>,
    api_key: Option<String>,
    hmac_credentials: Option<HmacCredentials>,
    additional_headers: HashMap<String, String>,
    logger: Option<SharedLogger>,
}

impl HttpClientBuilder {
    #[must_use]
    pub fn base_url(mut self, value: impl Into<String>) -> Self {
        self.base_url = Some(value.into());
        self
    }

    #[must_use]
    pub fn timeout(mut self, value: Duration) -> Self {
        self.timeout = Some(value);
        self
    }

    #[must_use]
    pub fn api_key(mut self, value: impl Into<String>) -> Self {
        self.api_key = Some(value.into());
        self
    }

    #[must_use]
    pub fn hmac_credentials(mut self, value: HmacCredentials) -> Self {
        self.hmac_credentials = Some(value);
        self
    }

    #[must_use]
    pub fn logger(mut self, value: SharedLogger) -> Self {
        self.logger = Some(value);
        self
    }

    #[must_use]
    pub fn additional_headers(mut self, headers: HashMap<String, String>) -> Self {
        self.additional_headers.extend(headers);
        self
    }

    pub fn build(self) -> Result<HttpClient> {
        let mut headers = sdk_tracking_headers();
        headers.insert("Accept".into(), "application/json".into());
        headers.extend(self.additional_headers);

        let reqwest = reqwest::Client::builder()
            .timeout(self.timeout.unwrap_or(Duration::from_secs(30)))
            .pool_idle_timeout(Duration::from_secs(60))
            .pool_max_idle_per_host(50)
            .build()?;

        let api_key = self
            .api_key
            .or_else(|| env::var("LIMITLESS_API_KEY").ok())
            .filter(|value| !value.trim().is_empty());
        let logger = self.logger.unwrap_or_else(noop_logger);

        if api_key.is_none() && self.hmac_credentials.is_none() {
            logger.warn(
                "Authentication not set. Authenticated endpoints will fail until an API key or HMAC credentials are configured.",
            );
        }

        Ok(HttpClient {
            inner: Arc::new(HttpClientInner {
                client: reqwest,
                state: RwLock::new(HttpClientState {
                    base_url: self
                        .base_url
                        .unwrap_or_else(|| DEFAULT_API_URL.to_string())
                        .trim_end_matches('/')
                        .to_string(),
                    api_key,
                    hmac_credentials: self.hmac_credentials,
                    headers,
                    logger,
                }),
            }),
        })
    }
}

impl HttpClient {
    #[must_use]
    pub fn builder() -> HttpClientBuilder {
        HttpClientBuilder::default()
    }

    pub fn set_api_key(&self, key: impl Into<String>) {
        self.write_state().api_key = Some(key.into());
    }

    pub fn clear_api_key(&self) {
        self.write_state().api_key = None;
    }

    pub fn api_key(&self) -> Option<String> {
        self.read_state().api_key.clone()
    }

    pub fn set_hmac_credentials(&self, creds: HmacCredentials) {
        self.write_state().hmac_credentials = Some(creds);
    }

    pub fn clear_hmac_credentials(&self) {
        self.write_state().hmac_credentials = None;
    }

    pub fn hmac_credentials(&self) -> Option<HmacCredentials> {
        self.read_state().hmac_credentials.clone()
    }

    pub fn base_url(&self) -> String {
        self.read_state().base_url.clone()
    }

    pub fn logger(&self) -> SharedLogger {
        self.read_state().logger.clone()
    }

    pub fn require_auth(&self, operation: &str) -> Result<()> {
        if self.api_key().is_some() || self.hmac_credentials().is_some() {
            return Ok(());
        }
        Err(LimitlessError::AuthenticationRequired {
            operation: operation.to_string(),
        })
    }

    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        self.do_request(
            Method::GET,
            path,
            Option::<&()>::None,
            RequestExecutionConfig::default(),
        )
        .await
    }

    pub async fn get_raw(&self, path: &str, options: RequestOptions) -> Result<RawResponse> {
        self.do_request_raw(
            Method::GET,
            path,
            Option::<&()>::None,
            options,
            RequestExecutionConfig::default(),
        )
        .await
    }

    pub async fn post<B: Serialize + ?Sized, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T> {
        self.do_request(
            Method::POST,
            path,
            Some(body),
            RequestExecutionConfig::default(),
        )
        .await
    }

    pub async fn patch<B: Serialize + ?Sized, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T> {
        self.do_request(
            Method::PATCH,
            path,
            Some(body),
            RequestExecutionConfig::default(),
        )
        .await
    }

    pub async fn delete<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        self.do_request(
            Method::DELETE,
            path,
            Option::<&()>::None,
            RequestExecutionConfig::default(),
        )
        .await
    }

    pub async fn post_with_headers<B: Serialize + ?Sized, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
        extra_headers: HashMap<String, String>,
    ) -> Result<T> {
        self.do_request(
            Method::POST,
            path,
            Some(body),
            RequestExecutionConfig {
                extra_headers,
                identity_token: None,
            },
        )
        .await
    }

    pub async fn post_with_identity<B: Serialize + ?Sized, T: DeserializeOwned>(
        &self,
        path: &str,
        identity_token: &str,
        body: &B,
    ) -> Result<T> {
        self.do_request(
            Method::POST,
            path,
            Some(body),
            RequestExecutionConfig {
                extra_headers: HashMap::new(),
                identity_token: Some(identity_token.to_string()),
            },
        )
        .await
    }

    pub async fn get_with_identity<T: DeserializeOwned>(
        &self,
        path: &str,
        identity_token: &str,
    ) -> Result<T> {
        self.do_request(
            Method::GET,
            path,
            Option::<&()>::None,
            RequestExecutionConfig {
                extra_headers: HashMap::new(),
                identity_token: Some(identity_token.to_string()),
            },
        )
        .await
    }

    async fn do_request<B: Serialize + ?Sized, T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<&B>,
        cfg: RequestExecutionConfig,
    ) -> Result<T> {
        let raw = self
            .do_request_raw(method, path, body, RequestOptions::default(), cfg)
            .await?;
        raw.json()
    }

    async fn do_request_raw<B: Serialize + ?Sized>(
        &self,
        method: Method,
        path: &str,
        body: Option<&B>,
        options: RequestOptions,
        cfg: RequestExecutionConfig,
    ) -> Result<RawResponse> {
        let url = join_url(&self.base_url(), path)?;
        let signing_path = request_uri_for_signing(path)?;
        let logger = self.logger();

        let body_bytes = match body {
            Some(value) => Some(serde_json::to_vec(value)?),
            None => None,
        };

        let mut request = self.inner.client.request(method.clone(), url.clone());
        request =
            self.apply_headers(request, &method, &signing_path, body_bytes.as_deref(), &cfg)?;

        if let Some(bytes) = &body_bytes {
            request = request.body(bytes.clone());
        }

        logger.debug(&format!("→ {} {}", method.as_str(), url));
        let response = request.send().await?;
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = response.bytes().await?.to_vec();

        let allowed = status.is_success() || options.allowed_statuses.contains(&status.as_u16());
        if !allowed {
            logger.error(&format!(
                "✗ {} {} {}",
                status.as_u16(),
                method.as_str(),
                path
            ));
            return Err(parse_api_error(status, &bytes, path, method.as_str()));
        }

        logger.debug(&format!(
            "✓ {} {} {}",
            status.as_u16(),
            method.as_str(),
            path
        ));
        Ok(RawResponse {
            status: status.as_u16(),
            headers,
            body: bytes,
        })
    }

    fn apply_headers(
        &self,
        mut request: reqwest::RequestBuilder,
        method: &Method,
        signing_path: &str,
        body_bytes: Option<&[u8]>,
        cfg: &RequestExecutionConfig,
    ) -> Result<reqwest::RequestBuilder> {
        let state = self.read_state();
        for (key, value) in &state.headers {
            request = request.header(key, value);
        }
        for (key, value) in &cfg.extra_headers {
            request = request.header(key, value);
        }

        if method != Method::DELETE && body_bytes.is_some() {
            request = request.header("Content-Type", "application/json");
        }

        if let Some(identity_token) = &cfg.identity_token {
            request = request.header("identity", format!("Bearer {identity_token}"));
            return Ok(request);
        }

        if let Some(creds) = &state.hmac_credentials {
            let body = body_bytes
                .map(|bytes| String::from_utf8_lossy(bytes).to_string())
                .unwrap_or_default();
            let timestamp = chrono_timestamp();
            let signature = compute_hmac_signature(
                &creds.secret,
                &timestamp,
                method.as_str(),
                signing_path,
                &body,
            )?;
            request = request
                .header("lmts-api-key", &creds.token_id)
                .header("lmts-timestamp", timestamp)
                .header("lmts-signature", signature);
            return Ok(request);
        }

        if let Some(api_key) = &state.api_key {
            request = request.header("X-API-Key", api_key);
        }

        Ok(request)
    }

    fn read_state(&self) -> std::sync::RwLockReadGuard<'_, HttpClientState> {
        self.inner
            .state
            .read()
            .unwrap_or_else(|err| err.into_inner())
    }

    fn write_state(&self) -> std::sync::RwLockWriteGuard<'_, HttpClientState> {
        self.inner
            .state
            .write()
            .unwrap_or_else(|err| err.into_inner())
    }
}

impl fmt::Debug for HttpClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = self.read_state();
        f.debug_struct("HttpClient")
            .field("base_url", &state.base_url)
            .field("has_api_key", &state.api_key.is_some())
            .field("has_hmac_credentials", &state.hmac_credentials.is_some())
            .finish()
    }
}

#[derive(Default)]
struct RequestExecutionConfig {
    extra_headers: HashMap<String, String>,
    identity_token: Option<String>,
}

fn sdk_tracking_headers() -> HashMap<String, String> {
    let version = env!("CARGO_PKG_VERSION");
    HashMap::from([
        ("user-agent".into(), format!("{SDK_ID}/{version}")),
        ("x-sdk-version".into(), format!("{SDK_ID}/{version}")),
    ])
}

fn join_url(base_url: &str, path: &str) -> Result<Url> {
    if path.starts_with("http://") || path.starts_with("https://") {
        return Url::parse(path).map_err(|err| LimitlessError::invalid_input(err.to_string()));
    }

    let base = if base_url.is_empty() {
        DEFAULT_API_URL
    } else {
        base_url
    };
    let joined = format!(
        "{}/{}",
        base.trim_end_matches('/'),
        path.trim_start_matches('/')
    );
    Url::parse(&joined).map_err(|err| LimitlessError::invalid_input(err.to_string()))
}

fn request_uri_for_signing(path: &str) -> Result<String> {
    if path.starts_with("http://") || path.starts_with("https://") {
        let url = Url::parse(path).map_err(|err| LimitlessError::invalid_input(err.to_string()))?;
        let mut request_uri = url.path().to_string();
        if let Some(query) = url.query() {
            request_uri.push('?');
            request_uri.push_str(query);
        }
        return Ok(request_uri);
    }

    if path.starts_with('/') {
        Ok(path.to_string())
    } else {
        Ok(format!("/{}", path))
    }
}

#[cfg(test)]
mod tests {
    use super::request_uri_for_signing;

    #[test]
    fn request_uri_uses_path_and_query() {
        let value = request_uri_for_signing("/markets/active?limit=10").unwrap();
        assert_eq!(value, "/markets/active?limit=10");
    }
}
