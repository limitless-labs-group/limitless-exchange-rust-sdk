use std::{fmt, future::Future, sync::Arc, time::Duration};

use serde::{de::DeserializeOwned, Serialize};

use crate::{
    errors::{ApiError, LimitlessError, Result},
    hmac::HmacCredentials,
    http_client::{HttpClient, RawResponse, RequestOptions},
    logger::{noop_logger, SharedLogger},
};

pub type RetryCallback = Arc<dyn Fn(usize, &LimitlessError, Duration) + Send + Sync>;

#[derive(Clone)]
pub struct RetryConfig {
    pub status_codes: Vec<u16>,
    pub max_retries: usize,
    pub delays: Vec<Duration>,
    pub exponential_base: f64,
    pub max_delay: Duration,
    pub on_retry: Option<RetryCallback>,
}

impl fmt::Debug for RetryConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RetryConfig")
            .field("status_codes", &self.status_codes)
            .field("max_retries", &self.max_retries)
            .field("delays", &self.delays)
            .field("exponential_base", &self.exponential_base)
            .field("max_delay", &self.max_delay)
            .field("has_on_retry", &self.on_retry.is_some())
            .finish()
    }
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            status_codes: vec![429, 500, 502, 503, 504],
            max_retries: 3,
            delays: Vec::new(),
            exponential_base: 2.0,
            max_delay: Duration::from_secs(60),
            on_retry: None,
        }
    }
}

impl RetryConfig {
    pub fn delay_for_attempt(&self, attempt: usize) -> Duration {
        if !self.delays.is_empty() {
            return self.delays[attempt.min(self.delays.len() - 1)];
        }

        let base = if self.exponential_base.is_finite() && self.exponential_base > 0.0 {
            self.exponential_base
        } else {
            2.0
        };
        let max_secs = self.max_delay.as_secs_f64();
        if max_secs <= 0.0 {
            return Duration::ZERO;
        }

        let exponent = attempt.min(63) as i32;
        let seconds = base.powi(exponent).min(max_secs);
        Duration::from_secs_f64(seconds)
    }

    pub fn should_retry(&self, error: &LimitlessError) -> bool {
        match error {
            LimitlessError::Api(ApiError { status, .. }) => self.status_codes.contains(status),
            LimitlessError::Request(err) => err.is_connect() || err.is_timeout(),
            _ => false,
        }
    }

    #[must_use]
    pub fn with_on_retry<F>(mut self, callback: F) -> Self
    where
        F: Fn(usize, &LimitlessError, Duration) + Send + Sync + 'static,
    {
        self.on_retry = Some(Arc::new(callback));
        self
    }
}

#[derive(Clone)]
pub struct RetryableClient {
    client: HttpClient,
    config: RetryConfig,
    logger: SharedLogger,
}

impl RetryableClient {
    pub fn new(client: HttpClient, config: RetryConfig, logger: Option<SharedLogger>) -> Self {
        Self {
            client,
            config,
            logger: logger.unwrap_or_else(noop_logger),
        }
    }

    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let client = self.client.clone();
        self.run(move || {
            let client = client.clone();
            async move { client.get(path).await }
        })
        .await
    }

    pub async fn get_raw(&self, path: &str, options: RequestOptions) -> Result<RawResponse> {
        let client = self.client.clone();
        self.run(move || {
            let client = client.clone();
            let options = options.clone();
            async move { client.get_raw(path, options).await }
        })
        .await
    }

    pub async fn post<B: Serialize + ?Sized, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T> {
        let client = self.client.clone();
        self.run(move || {
            let client = client.clone();
            async move { client.post(path, body).await }
        })
        .await
    }

    pub async fn patch<B: Serialize + ?Sized, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T> {
        let client = self.client.clone();
        self.run(move || {
            let client = client.clone();
            async move { client.patch(path, body).await }
        })
        .await
    }

    pub async fn delete<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let client = self.client.clone();
        self.run(move || {
            let client = client.clone();
            async move { client.delete(path).await }
        })
        .await
    }

    pub async fn delete_with_identity(&self, path: &str, identity_token: &str) -> Result<()> {
        let client = self.client.clone();
        self.run(move || {
            let client = client.clone();
            async move { client.delete_with_identity(path, identity_token).await }
        })
        .await
    }

    pub fn set_api_key(&self, key: impl Into<String>) {
        self.client.set_api_key(key);
    }

    pub fn clear_api_key(&self) {
        self.client.clear_api_key();
    }

    pub fn set_hmac_credentials(&self, creds: HmacCredentials) {
        self.client.set_hmac_credentials(creds);
    }

    pub fn clear_hmac_credentials(&self) {
        self.client.clear_hmac_credentials();
    }

    pub fn hmac_credentials(&self) -> Option<HmacCredentials> {
        self.client.hmac_credentials()
    }

    pub async fn post_with_headers<B: Serialize + ?Sized, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
        extra_headers: std::collections::HashMap<String, String>,
    ) -> Result<T> {
        let client = self.client.clone();
        self.run(move || {
            let client = client.clone();
            let extra_headers = extra_headers.clone();
            async move { client.post_with_headers(path, body, extra_headers).await }
        })
        .await
    }

    pub async fn post_with_identity<B: Serialize + ?Sized, T: DeserializeOwned>(
        &self,
        path: &str,
        identity_token: &str,
        body: &B,
    ) -> Result<T> {
        let client = self.client.clone();
        self.run(move || {
            let client = client.clone();
            async move { client.post_with_identity(path, identity_token, body).await }
        })
        .await
    }

    pub async fn get_with_identity<T: DeserializeOwned>(
        &self,
        path: &str,
        identity_token: &str,
    ) -> Result<T> {
        let client = self.client.clone();
        self.run(move || {
            let client = client.clone();
            async move { client.get_with_identity(path, identity_token).await }
        })
        .await
    }

    async fn run<T, F, Fut>(&self, operation: F) -> Result<T>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        with_retry(self.config.clone(), Some(self.logger.clone()), operation).await
    }
}

pub async fn with_retry<T, F, Fut>(
    config: RetryConfig,
    logger: Option<SharedLogger>,
    mut operation: F,
) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    let logger = logger.unwrap_or_else(noop_logger);
    let mut last_error = None;

    for attempt in 0..=config.max_retries {
        match operation().await {
            Ok(value) => return Ok(value),
            Err(err) => {
                let retryable = config.should_retry(&err);
                last_error = Some(err);

                if !retryable || attempt == config.max_retries {
                    break;
                }

                let delay = config.delay_for_attempt(attempt);
                if let (Some(callback), Some(err)) = (config.on_retry.as_ref(), last_error.as_ref())
                {
                    callback(attempt, err, delay);
                }

                logger.warn(&format!(
                    "Retrying request after failure (attempt {} of {})",
                    attempt + 1,
                    config.max_retries
                ));
                tokio::time::sleep(delay).await;
            }
        }
    }

    Err(last_error.expect("retry loop always stores the last error"))
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc, Mutex,
        },
        time::Duration,
    };

    use super::{with_retry, RetryConfig, RetryableClient};
    use crate::{
        errors::{ApiError, LimitlessError},
        hmac::HmacCredentials,
        http_client::HttpClient,
    };

    #[tokio::test]
    async fn retries_and_eventually_succeeds() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let attempts_ref = attempts.clone();

        let result = with_retry(
            RetryConfig {
                status_codes: vec![429],
                max_retries: 3,
                delays: vec![Duration::from_millis(1); 3],
                ..RetryConfig::default()
            },
            None,
            move || {
                let attempts_ref = attempts_ref.clone();
                async move {
                    let attempt = attempts_ref.fetch_add(1, Ordering::SeqCst);
                    if attempt < 2 {
                        Err(LimitlessError::Api(ApiError {
                            status: 429,
                            message: "slow down".to_string(),
                            data: serde_json::Value::Null,
                            url: "/test".to_string(),
                            method: "GET".to_string(),
                        }))
                    } else {
                        Ok("ok")
                    }
                }
            },
        )
        .await
        .unwrap();

        assert_eq!(result, "ok");
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn does_not_retry_non_retryable_error() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let attempts_ref = attempts.clone();

        let error = with_retry(
            RetryConfig {
                status_codes: vec![429],
                max_retries: 3,
                delays: vec![Duration::from_millis(1); 3],
                ..RetryConfig::default()
            },
            None,
            move || {
                let attempts_ref = attempts_ref.clone();
                async move {
                    attempts_ref.fetch_add(1, Ordering::SeqCst);
                    Err::<(), _>(LimitlessError::invalid_input("boom"))
                }
            },
        )
        .await
        .unwrap_err();

        assert!(matches!(error, LimitlessError::InvalidInput(_)));
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn on_retry_callback_runs_before_each_retry() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let calls_ref = calls.clone();
        let attempts = Arc::new(AtomicUsize::new(0));
        let attempts_ref = attempts.clone();

        let config = RetryConfig {
            status_codes: vec![429],
            max_retries: 2,
            delays: vec![Duration::from_millis(1), Duration::from_millis(2)],
            ..RetryConfig::default()
        }
        .with_on_retry(move |attempt, error, delay| {
            calls_ref
                .lock()
                .unwrap()
                .push((attempt, error.to_string(), delay));
        });

        let result = with_retry(config, None, move || {
            let attempts_ref = attempts_ref.clone();
            async move {
                let attempt = attempts_ref.fetch_add(1, Ordering::SeqCst);
                if attempt == 0 {
                    Err(LimitlessError::Api(ApiError {
                        status: 429,
                        message: "retry me".to_string(),
                        data: serde_json::Value::Null,
                        url: "/retry".to_string(),
                        method: "GET".to_string(),
                    }))
                } else {
                    Ok("ok")
                }
            }
        })
        .await
        .unwrap();

        let calls = calls.lock().unwrap();
        assert_eq!(result, "ok");
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, 0);
        assert!(calls[0].1.contains("retry me"));
        assert_eq!(calls[0].2, Duration::from_millis(1));
    }

    #[test]
    fn delay_for_attempt_clamps_before_duration_overflow() {
        let config = RetryConfig {
            exponential_base: 2.0,
            max_delay: Duration::from_secs(60),
            ..RetryConfig::default()
        };

        assert_eq!(config.delay_for_attempt(10_000), Duration::from_secs(60));
    }

    #[test]
    fn retryable_client_forwards_auth_mutators() {
        let client = HttpClient::builder().build().unwrap();
        let retryable = RetryableClient::new(client, RetryConfig::default(), None);

        retryable.set_api_key("key");
        assert_eq!(retryable.client.api_key().as_deref(), Some("key"));

        let creds = HmacCredentials {
            token_id: "token".to_string(),
            secret: "secret".to_string(),
        };
        retryable.set_hmac_credentials(creds.clone());
        assert_eq!(retryable.hmac_credentials(), Some(creds));

        retryable.clear_api_key();
        retryable.clear_hmac_credentials();
        assert_eq!(retryable.client.api_key(), None);
        assert_eq!(retryable.hmac_credentials(), None);
    }
}
