//! Partner curated Pack wire types and workflows. Monetary values are decimal strings.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PackLeg {
    pub id: Option<String>,
    #[serde(rename = "legIndex")]
    pub leg_index: Option<i32>,
    #[serde(rename = "priceAtCreation")]
    pub price_at_creation: Option<String>,
    #[serde(rename = "resolutionStatus")]
    pub resolution_status: Option<String>,
    #[serde(rename = "resolvedAt")]
    pub resolved_at: Option<String>,
    #[serde(rename = "marketSlug")]
    pub market_slug: Option<String>,
    #[serde(rename = "marketId")]
    pub market_id: Option<i32>,
    #[serde(rename = "conditionId")]
    pub condition_id: Option<String>,
    #[serde(rename = "marketType")]
    pub market_type: Option<String>,
    #[serde(rename = "outcome")]
    pub outcome: String,
    #[serde(rename = "price")]
    pub price: Option<f64>,
    #[serde(rename = "bookPrice")]
    pub book_price: Option<f64>,
    #[serde(rename = "livePrice")]
    pub live_price: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Pack {
    #[serde(rename = "id")]
    pub id: String,
    #[serde(rename = "name")]
    pub name: String,
    #[serde(rename = "status")]
    pub status: String,
    #[serde(rename = "eligible")]
    pub eligible: bool,
    #[serde(rename = "parlayType")]
    pub parlay_type: String,
    #[serde(rename = "legs")]
    pub legs: Vec<PackLeg>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PackQuoteParams {
    #[serde(rename = "packId")]
    pub pack_id: String,
    #[serde(rename = "stakeAmount")]
    pub stake_amount: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PackQuote {
    #[serde(rename = "eligible")]
    pub eligible: bool,
    #[serde(rename = "parlayType")]
    pub parlay_type: Option<String>,
    #[serde(rename = "rawMultiplier")]
    pub raw_multiplier: Option<f64>,
    #[serde(rename = "multiplier")]
    pub multiplier: Option<f64>,
    #[serde(rename = "potentialPayout")]
    pub potential_payout: Option<String>,
    #[serde(rename = "collateralRequired")]
    pub collateral_required: Option<String>,
    #[serde(rename = "vigPercent")]
    pub vig_percent: Option<f64>,
    #[serde(rename = "legs")]
    pub legs: Option<Vec<PackLeg>>,
    #[serde(rename = "ineligibleReasons")]
    pub ineligible_reasons: Option<Vec<String>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PackBuyParams {
    #[serde(rename = "onBehalfOf")]
    pub on_behalf_of: i32,
    #[serde(rename = "packId")]
    pub pack_id: String,
    #[serde(rename = "stakeAmount")]
    pub stake_amount: String,
    #[serde(rename = "minPayoutAmount")]
    pub min_payout_amount: String,
    #[serde(rename = "deadline")]
    pub deadline: String,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PackSource {
    #[serde(rename = "id")]
    pub id: String,
    #[serde(rename = "name")]
    pub name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PackPosition {
    #[serde(rename = "id")]
    pub id: String,
    #[serde(rename = "status")]
    pub status: String,
    #[serde(rename = "stakeAmount")]
    pub stake_amount: String,
    #[serde(rename = "collateralDeposited")]
    pub collateral_deposited: String,
    #[serde(rename = "multiplierOffered")]
    pub multiplier_offered: String,
    #[serde(rename = "vigPercent")]
    pub vig_percent: String,
    #[serde(rename = "txHash")]
    pub tx_hash: Option<String>,
    #[serde(rename = "claimTxHash")]
    pub claim_tx_hash: Option<String>,
    #[serde(rename = "wallet")]
    pub wallet: String,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "legs")]
    pub legs: Vec<PackLeg>,
    #[serde(rename = "sourcePack")]
    pub source_pack: Option<PackSource>,
    #[serde(rename = "payoutAmount")]
    pub payout_amount: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PackBuyResponse {
    #[serde(rename = "status")]
    pub status: String,
    #[serde(rename = "position")]
    pub position: PackPosition,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PackPositions {
    #[serde(rename = "data")]
    pub data: Vec<PackPosition>,
    #[serde(rename = "total")]
    pub total: i32,
    #[serde(rename = "limit")]
    pub limit: i32,
    #[serde(rename = "offset")]
    pub offset: i32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PackClaimResponse {
    #[serde(rename = "parlayId")]
    pub parlay_id: String,
    #[serde(rename = "status")]
    pub status: String,
    #[serde(rename = "txHash")]
    pub tx_hash: Option<String>,
    #[serde(rename = "transactionId")]
    pub transaction_id: Option<String>,
    #[serde(rename = "userOperationHash")]
    pub user_operation_hash: Option<String>,
}

use crate::{
    errors::{LimitlessError, Result},
    HttpClient, RequestOptions,
};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::{
    sync::Mutex,
    time::{sleep, sleep_until, Instant},
};

/// Reuse one instance per partner. Clones share the request queue and pacing.
/// Persist PackBuyParams before buy(); no write is retried automatically.
#[derive(Clone)]
pub struct PacksService {
    client: HttpClient,
    next: Arc<Mutex<Instant>>,
}
impl PacksService {
    pub fn new(client: HttpClient) -> Self {
        Self {
            client,
            next: Arc::new(Mutex::new(Instant::now())),
        }
    }
    async fn call<T: DeserializeOwned>(
        &self,
        path: &str,
        body: Option<Value>,
        profile: Option<i32>,
    ) -> Result<T> {
        if path.starts_with("/parlay/partner/") && self.client.hmac_credentials().is_none() {
            return Err(LimitlessError::invalid_input(
                "partner Packs require HMAC credentials with trading and delegated_signing scopes",
            ));
        }
        let mut next = self.next.lock().await;
        sleep_until(*next).await;
        *next = Instant::now() + Duration::from_millis(1100);
        let result = if let Some(body) = body {
            self.client
                .post_raw(path, &body, RequestOptions::default())
                .await
        } else {
            let headers = profile
                .map(|p| HashMap::from([("x-on-behalf-of".into(), p.to_string())]))
                .unwrap_or_default();
            self.client
                .get_raw_with_headers(path, headers, RequestOptions::default())
                .await
        };
        if matches!(&result, Err(LimitlessError::Api(e)) if e.status == 429) {
            *next = Instant::now() + Duration::from_secs(10);
        }
        result?.json()
    }
    pub async fn list(&self) -> Result<Vec<Pack>> {
        self.call("/parlay/packs", None, None).await
    }
    pub async fn search(&self, query: &str) -> Result<Vec<Pack>> {
        self.call(
            &format!("/parlay/packs/search?query={}", urlencoding::encode(query)),
            None,
            None,
        )
        .await
    }
    pub async fn get(&self, pack_id: &str) -> Result<Pack> {
        validate_id(pack_id)?;
        self.call(&format!("/parlay/packs/{pack_id}"), None, None)
            .await
    }
    pub async fn quote(&self, params: &PackQuoteParams) -> Result<PackQuote> {
        validate_id(&params.pack_id)?;
        validate_amount(&params.stake_amount)?;
        self.call(
            "/parlay/partner/quote",
            Some(serde_json::to_value(params)?),
            None,
        )
        .await
    }
    pub async fn buy(&self, params: &PackBuyParams) -> Result<PackBuyResponse> {
        validate_buy(params)?;
        self.call(
            "/parlay/partner/buy",
            Some(serde_json::to_value(params)?),
            None,
        )
        .await
    }
    pub async fn list_positions(
        &self,
        on_behalf_of: i32,
        limit: i32,
        offset: i32,
    ) -> Result<PackPositions> {
        validate_profile(on_behalf_of)?;
        self.call(
            &format!(
                "/parlay/partner/positions?limit={}&offset={}",
                limit.clamp(1, 50),
                offset.clamp(0, 5000)
            ),
            None,
            Some(on_behalf_of),
        )
        .await
    }
    pub async fn get_position(&self, on_behalf_of: i32, position_id: &str) -> Result<PackPosition> {
        validate_profile(on_behalf_of)?;
        validate_id(position_id)?;
        self.call(
            &format!("/parlay/partner/positions/{position_id}"),
            None,
            Some(on_behalf_of),
        )
        .await
    }
    /// Wait for minting, not market settlement. Dropping the future cancels polling.
    pub async fn wait_for_position(
        &self,
        on_behalf_of: i32,
        position_id: &str,
        options: PackPollOptions,
    ) -> Result<PackPosition> {
        options.validate()?;
        for attempt in 0..options.max_attempts {
            let result = self.get_position(on_behalf_of, position_id).await?;
            match result.status.as_str() {
                "ACTIVE" | "WON" | "LOST" => return Ok(result),
                "PENDING" | "FAILED" => {}
                _ => {
                    return Err(LimitlessError::invalid_input(format!(
                        "Pack position {position_id} is {}",
                        result.status
                    )))
                }
            }
            if attempt + 1 < options.max_attempts {
                sleep(options.interval).await;
            }
        }
        Err(LimitlessError::invalid_input(
            "Pack mint polling exhausted; replay the identical saved buy to distinguish NOT_SUBMITTED from recovery; do not rotate its key",
        ))
    }
    pub async fn claim_once(
        &self,
        on_behalf_of: i32,
        position_id: &str,
    ) -> Result<PackClaimResponse> {
        validate_profile(on_behalf_of)?;
        validate_id(position_id)?;
        self.call(
            &format!("/parlay/partner/positions/{position_id}/claim"),
            Some(json!({"onBehalfOf": on_behalf_of})),
            None,
        )
        .await
    }
    /// Stops on every error, including PACK_SUBMISSION_UNKNOWN. Use confirm_claim with a mined receipt.
    pub async fn claim(
        &self,
        on_behalf_of: i32,
        position_id: &str,
        options: PackPollOptions,
    ) -> Result<PackClaimResponse> {
        options.validate()?;
        for attempt in 0..options.max_attempts {
            let result = self.claim_once(on_behalf_of, position_id).await?;
            match result.status.as_str() {
                "CLAIMED" => return Ok(result),
                "APPROVAL_PENDING" | "CLAIM_PENDING" => {}
                _ => {
                    return Err(LimitlessError::invalid_input(format!(
                        "unknown Pack claim status {}",
                        result.status
                    )))
                }
            }
            if attempt + 1 < options.max_attempts {
                sleep(options.interval).await;
            }
        }
        Err(LimitlessError::invalid_input(
            "Pack polling exhausted; resume with the same position/request",
        ))
    }
    pub async fn confirm_claim(
        &self,
        on_behalf_of: i32,
        position_id: &str,
        tx_hash: &str,
    ) -> Result<PackClaimResponse> {
        validate_profile(on_behalf_of)?;
        validate_id(position_id)?;
        if !regex::Regex::new(r"^0x[0-9a-fA-F]{64}$")
            .unwrap()
            .is_match(tx_hash)
        {
            return Err(LimitlessError::invalid_input(
                "tx_hash must be a mined transaction hash",
            ));
        }
        self.call(
            &format!("/parlay/partner/positions/{position_id}/claim/confirm"),
            Some(json!({"onBehalfOf": on_behalf_of, "txHash": tx_hash})),
            None,
        )
        .await
    }
}
#[derive(Clone, Copy, Debug)]
pub struct PackPollOptions {
    pub max_attempts: usize,
    pub interval: Duration,
}
impl Default for PackPollOptions {
    fn default() -> Self {
        Self {
            max_attempts: 60,
            interval: Duration::from_secs(5),
        }
    }
}
impl PackPollOptions {
    fn validate(&self) -> Result<()> {
        if self.max_attempts == 0 || self.interval < Duration::from_secs(1) {
            return Err(LimitlessError::invalid_input(
                "polling requires positive max_attempts and interval >= 1 second",
            ));
        }
        Ok(())
    }
}
/// Floor the slippage-adjusted payout using integer arithmetic only.
pub fn pack_min_payout(payout: &str, slippage_bps: u32) -> Result<String> {
    validate_amount(payout)?;
    if slippage_bps >= 10000 {
        return Err(LimitlessError::invalid_input(
            "slippage_bps must be between 0 and 9999",
        ));
    }
    // Inputs have at most 20 digits; the product fits comfortably in u128.
    let result = payout.parse::<u128>().unwrap() * u128::from(10000 - slippage_bps) / 10000;
    if result == 0 {
        return Err(LimitlessError::invalid_input(
            "minimum payout rounds to zero",
        ));
    }
    Ok(result.to_string())
}
fn validate_amount(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 20
        || value.starts_with('0')
        || !value.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(LimitlessError::invalid_input(
            "Pack amounts and deadlines must be positive integer strings of at most 20 digits",
        ));
    }
    Ok(())
}
fn validate_id(value: &str) -> Result<()> {
    if !regex::Regex::new(
        r"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$",
    )
    .unwrap()
    .is_match(value)
    {
        return Err(LimitlessError::invalid_input(
            "Pack/position ID must be a UUID",
        ));
    }
    Ok(())
}
fn validate_profile(value: i32) -> Result<()> {
    if value < 1 {
        return Err(LimitlessError::invalid_input(
            "on_behalf_of must be a positive 32-bit integer",
        ));
    }
    Ok(())
}
fn validate_buy(params: &PackBuyParams) -> Result<()> {
    validate_profile(params.on_behalf_of)?;
    validate_id(&params.pack_id)?;
    for value in [
        &params.stake_amount,
        &params.min_payout_amount,
        &params.deadline,
    ] {
        validate_amount(value)?;
    }
    if params.deadline.len() > 12 {
        return Err(LimitlessError::invalid_input(
            "deadline must contain at most 12 digits",
        ));
    }
    // An expired deadline is valid for replay; leave first-submission time validation to the API.
    if params.idempotency_key.is_empty()
        || params.idempotency_key.len() > 128
        || !params
            .idempotency_key
            .bytes()
            .all(|b| (33..=126).contains(&b))
    {
        return Err(LimitlessError::invalid_input(
            "idempotency_key must contain 1 to 128 printable ASCII characters without spaces",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn service(base: String) -> PacksService {
        PacksService::new(
            HttpClient::builder()
                .base_url(base)
                .hmac_credentials(crate::HmacCredentials {
                    token_id: "test".into(),
                    secret: "c2VjcmV0".into(),
                })
                .build()
                .unwrap(),
        )
    }
    fn position(status: &str) -> String {
        json!({"id":params().pack_id,"status":status,"stakeAmount":"1000000","collateralDeposited":"2000000","multiplierOffered":"2.000000","vigPercent":"10.00","wallet":"0xwallet","createdAt":"2026-10-18T00:00:00Z","legs":[],"txHash":"0xbroadcast"}).to_string()
    }
    #[tokio::test]
    async fn ineligible_quote_over_http() {
        let (base, _) = crate::test_support::spawn_routes(vec![(
            "POST",
            "/parlay/partner/quote",
            200,
            json!({"eligible":false,"ineligibleReasons":["Insufficient depth"]}).to_string(),
        )]);
        let quote = service(base)
            .quote(&PackQuoteParams {
                pack_id: params().pack_id,
                stake_amount: "1000000".into(),
            })
            .await
            .unwrap();
        assert!(!quote.eligible);
        assert_eq!(
            quote.ineligible_reasons.unwrap(),
            vec!["Insufficient depth"]
        );
        assert!(quote.potential_payout.is_none());
        assert!(quote.legs.is_none());
    }
    #[tokio::test]
    async fn failed_position_recovers_and_exhaustion_retains_purchase() {
        const PATH: &str = "/parlay/partner/positions/22222222-2222-4222-8222-222222222222";
        let (base, _requests) = crate::test_support::spawn_sequence(vec![
            ("GET", PATH, 200, position("FAILED")),
            ("GET", PATH, 200, position("ACTIVE")),
            ("GET", PATH, 200, position("FAILED")),
            ("GET", PATH, 200, position("VAULT_REVERTED")),
        ]);
        let s = service(base);
        let options = || PackPollOptions {
            max_attempts: 2,
            interval: Duration::from_secs(1),
        };
        assert_eq!(
            s.wait_for_position(123, &params().pack_id, options())
                .await
                .unwrap()
                .status,
            "ACTIVE"
        );
        let single = PackPollOptions {
            max_attempts: 1,
            interval: Duration::from_secs(1),
        };
        assert!(s
            .wait_for_position(123, &params().pack_id, single)
            .await
            .unwrap_err()
            .to_string()
            .contains("replay the identical saved buy"));
        assert!(s
            .wait_for_position(123, &params().pack_id, options())
            .await
            .unwrap_err()
            .to_string()
            .contains("VAULT_REVERTED"));
    }
    #[tokio::test]
    async fn rate_limit_defers_next_request() {
        let (base, requests) = crate::test_support::spawn_routes(vec![(
            "GET",
            "/parlay/packs",
            429,
            json!({"message":"rate limited","retryAfterSeconds":10}).to_string(),
        )]);
        let s = service(base);
        assert!(matches!(s.list().await, Err(LimitlessError::Api(e)) if e.status == 429));
        requests.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(tokio::time::timeout(Duration::from_secs(2), s.list())
            .await
            .is_err());
        assert!(requests.try_recv().is_err());
    }
    #[tokio::test]
    async fn claim_errors_stop_without_automatic_resubmission() {
        for (status, body) in [
            (
                409,
                json!({"code":"PACK_SUBMISSION_UNKNOWN","message":"Find receipt"}),
            ),
            (
                409,
                json!({"code":"PACK_TRANSACTION_REVERTED","message":"Reverted"}),
            ),
            (
                409,
                json!({"message":"Another Pack operation is in progress for this wallet; retry the same request"}),
            ),
            (
                429,
                json!({"message":"rate limited","retryAfterSeconds":10}),
            ),
        ] {
            let (base, requests) = crate::test_support::spawn_routes(vec![(
                "POST",
                "/parlay/partner/positions/22222222-2222-4222-8222-222222222222/claim",
                status,
                body.to_string(),
            )]);
            let error = service(base)
                .claim(123, &params().pack_id, PackPollOptions::default())
                .await
                .unwrap_err();
            assert!(
                matches!(error, LimitlessError::Api(e) if e.status == status && e.data == body)
            );
            requests.recv_timeout(Duration::from_secs(1)).unwrap();
            assert!(requests.try_recv().is_err());
        }
    }
    fn params() -> PackBuyParams {
        PackBuyParams {
            on_behalf_of: 123,
            pack_id: "22222222-2222-4222-8222-222222222222".into(),
            stake_amount: "1000000".into(),
            min_payout_amount: "2000000".into(),
            deadline: "1".into(),
            idempotency_key: "order-1".into(),
        }
    }
    #[test]
    fn replay_and_precision() {
        let p = params();
        validate_buy(&p).unwrap(); // expired deadline remains replayable
        let body = serde_json::to_value(&p).unwrap();
        assert_eq!(body["onBehalfOf"], 123);
        assert_eq!(body["deadline"], "1");

        assert_eq!(
            pack_min_payout("99999999999999999999", 100).unwrap(),
            "98999999999999999999"
        );
    }
    #[test]
    fn rejects_invalid_amounts_and_keys() {
        for amount in ["0", "01", "1e6", "1.5", "999999999999999999999"] {
            let mut p = params();
            p.stake_amount = amount.into();
            assert!(validate_buy(&p).is_err());
        }
        for key in ["", "space key", "é", &"x".repeat(129)] {
            let mut p = params();
            p.idempotency_key = key.into();
            assert!(validate_buy(&p).is_err());
        }
    }
    #[tokio::test]
    async fn rejects_legacy_auth_before_network() {
        let service = PacksService::new(HttpClient::builder().api_key("legacy").build().unwrap());
        assert!(service
            .buy(&params())
            .await
            .unwrap_err()
            .to_string()
            .contains("HMAC"));
    }
    #[test]
    fn parses_nullable_quote_and_distinct_claim_identifiers() {
        let q: PackQuote = serde_json::from_value(json!({"eligible":true,"parlayType":"CLOB","rawMultiplier":2.0,"multiplier":1.8,"potentialPayout":"1800000","collateralRequired":"1800000","vigPercent":10,"legs":[],"ineligibleReasons":null})).unwrap();
        assert!(q.ineligible_reasons.is_none());
        let c: PackClaimResponse = serde_json::from_value(
            json!({"parlayId":"p","status":"CLAIM_PENDING","userOperationHash":"0xoperation"}),
        )
        .unwrap();
        assert!(c.tx_hash.is_none());
        assert_eq!(c.user_operation_hash.as_deref(), Some("0xoperation"));
    }
    #[tokio::test]
    async fn signs_pagination_and_delegation_headers_on_wire() {
        let (base, requests) = crate::test_support::spawn_routes(vec![(
            "GET",
            "/parlay/partner/positions",
            200,
            r#"{"data":[],"total":0,"limit":50,"offset":0}"#.into(),
        )]);
        let service = PacksService::new(
            HttpClient::builder()
                .base_url(base)
                .hmac_credentials(crate::HmacCredentials {
                    token_id: "test".into(),
                    secret: "c2VjcmV0".into(),
                })
                .build()
                .unwrap(),
        );
        let result = service.list_positions(123, 100, -1).await.unwrap();
        assert_eq!(result.limit, 50);
        let req = requests.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(req.path, "/parlay/partner/positions?limit=50&offset=0");
        assert_eq!(req.headers["x-on-behalf-of"], "123");
        let signature = crate::compute_hmac_signature(
            "c2VjcmV0",
            &req.headers["lmts-timestamp"],
            "GET",
            &req.path,
            "",
        )
        .unwrap();
        assert_eq!(req.headers["lmts-signature"], signature);
    }
    #[test]
    fn parses_position_decimal_strings_and_optional_leg_slug() {
        let p: PackPosition = serde_json::from_value(json!({"id":"position","status":"ACTIVE","stakeAmount":"1000000","collateralDeposited":"2000000","multiplierOffered":"2.000000","vigPercent":"10.00","wallet":"0xwallet","createdAt":"2026-10-18T00:00:00Z","legs":[{"marketId":1,"conditionId":"0xcondition","outcome":"YES","priceAtCreation":"0.54","resolutionStatus":"PENDING"}]})).unwrap();
        assert_eq!(p.multiplier_offered, "2.000000");
        assert_eq!(p.legs[0].price_at_creation.as_deref(), Some("0.54"));
    }
}
