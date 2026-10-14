use std::{collections::HashMap, sync::RwLock};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::form_urlencoded::Serializer;

use crate::{
    errors::Result,
    http_client::{HttpClient, RequestOptions},
    raw_response::SdkResponse,
};

#[derive(Clone)]
pub struct MarketFetcher {
    client: HttpClient,
    venue_cache: std::sync::Arc<RwLock<HashMap<String, Venue>>>,
}

impl MarketFetcher {
    pub fn new(client: HttpClient) -> Self {
        Self {
            client,
            venue_cache: Default::default(),
        }
    }

    pub async fn get_active_markets(
        &self,
        params: Option<&ActiveMarketsParams>,
    ) -> Result<ActiveMarketsResponse> {
        Ok(self.get_active_markets_with_raw(params).await?.data)
    }

    pub async fn get_active_markets_with_raw(
        &self,
        params: Option<&ActiveMarketsParams>,
    ) -> Result<SdkResponse<ActiveMarketsResponse>> {
        let endpoint = active_markets_endpoint(params);
        let raw = self
            .client
            .get_raw(&endpoint, RequestOptions::default())
            .await?;
        let mut data: ActiveMarketsResponse = raw.json()?;
        for market in &mut data.data {
            market.client = Some(self.client.clone());
        }
        Ok(SdkResponse { data, raw })
    }

    pub async fn get_market(&self, slug: &str) -> Result<Market> {
        Ok(self.get_market_with_raw(slug).await?.data)
    }

    pub async fn get_market_with_raw(&self, slug: &str) -> Result<SdkResponse<Market>> {
        let raw = self
            .client
            .get_raw(
                &format!("/markets/{}", urlencoding::encode(slug)),
                RequestOptions::default(),
            )
            .await?;
        let mut market: Market = raw.json()?;
        market.client = Some(self.client.clone());

        if let Some(venue) = market.venue.clone() {
            self.venue_cache
                .write()
                .unwrap_or_else(|err| err.into_inner())
                .insert(slug.to_string(), venue);
        }
        Ok(SdkResponse { data: market, raw })
    }

    /// Returns the cached venue for `slug`, or fetches the market to resolve it.
    pub(crate) async fn resolve_venue(&self, slug: &str) -> Result<Option<Venue>> {
        match self.get_venue(slug) {
            Some(venue) if !venue.exchange.is_empty() => Ok(Some(venue)),
            _ => Ok(self.get_market(slug).await?.venue),
        }
    }

    #[cfg(test)]
    pub(crate) fn cache_venue(&self, slug: &str, venue: Venue) {
        self.venue_cache
            .write()
            .unwrap_or_else(|err| err.into_inner())
            .insert(slug.to_string(), venue);
    }

    pub fn get_venue(&self, slug: &str) -> Option<Venue> {
        self.venue_cache
            .read()
            .unwrap_or_else(|err| err.into_inner())
            .get(slug)
            .cloned()
    }

    pub async fn get_order_book(&self, slug: &str) -> Result<OrderBook> {
        Ok(self.get_order_book_with_raw(slug).await?.data)
    }

    pub async fn get_order_book_with_raw(&self, slug: &str) -> Result<SdkResponse<OrderBook>> {
        let raw = self
            .client
            .get_raw(
                &format!("/markets/{}/orderbook", urlencoding::encode(slug)),
                RequestOptions::default(),
            )
            .await?;
        let data = raw.json()?;
        Ok(SdkResponse { data, raw })
    }

    pub async fn get_user_orders(&self, slug: &str) -> Result<Vec<UserOrder>> {
        Ok(self.get_user_orders_with_raw(slug).await?.data)
    }

    pub async fn get_user_orders_with_raw(
        &self,
        slug: &str,
    ) -> Result<SdkResponse<Vec<UserOrder>>> {
        self.client.require_auth("get_user_orders")?;
        let raw = self
            .client
            .get_raw(
                &format!("/markets/{}/user-orders", urlencoding::encode(slug)),
                RequestOptions::default(),
            )
            .await?;
        let data = raw.json()?;
        Ok(SdkResponse { data, raw })
    }
}

fn active_markets_endpoint(params: Option<&ActiveMarketsParams>) -> String {
    let mut endpoint = "/markets/active".to_string();
    if let Some(params) = params {
        let mut query = Serializer::new(String::new());
        if let Some(limit) = params.limit {
            query.append_pair("limit", &limit.to_string());
        }
        if let Some(page) = params.page {
            query.append_pair("page", &page.to_string());
        }
        if let Some(sort_by) = &params.sort_by {
            query.append_pair("sortBy", sort_by.as_ref());
        }
        let encoded = query.finish();
        if !encoded.is_empty() {
            endpoint.push('?');
            endpoint.push_str(&encoded);
        }
    }
    endpoint
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CollateralToken {
    pub address: String,
    pub decimals: i32,
    pub symbol: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MarketCreator {
    pub name: String,
    #[serde(rename = "imageURI", default)]
    pub image_uri: Option<String>,
    #[serde(default)]
    pub link: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MarketMetadata {
    pub fee: bool,
    #[serde(rename = "isBannered", default)]
    pub is_bannered: Option<bool>,
    #[serde(rename = "isPolyArbitrage", default)]
    pub is_poly_arbitrage: Option<bool>,
    #[serde(rename = "shouldMarketMake", default)]
    pub should_market_make: Option<bool>,
    #[serde(rename = "openPrice", default)]
    pub open_price: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MarketSettings {
    #[serde(rename = "minSize")]
    pub min_size: String,
    #[serde(rename = "maxSpread")]
    pub max_spread: Value,
    #[serde(rename = "dailyReward")]
    pub daily_reward: String,
    #[serde(rename = "rewardsEpoch")]
    pub rewards_epoch: Value,
    pub c: Value,
    #[serde(rename = "rebateRate", default)]
    pub rebate_rate: Option<Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TradePriceSide {
    pub market: [f64; 2],
    pub limit: [f64; 2],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TradePrices {
    pub buy: TradePriceSide,
    pub sell: TradePriceSide,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PriceOracleMetadata {
    pub ticker: String,
    #[serde(rename = "assetType")]
    pub asset_type: String,
    #[serde(rename = "pythAddress")]
    pub pyth_address: String,
    pub symbol: String,
    pub name: String,
    #[serde(default)]
    pub logo: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MarketOutcome {
    pub id: i32,
    pub title: String,
    #[serde(rename = "tokenId")]
    pub token_id: String,
    #[serde(default)]
    pub price: Option<f64>,
}

/// `venue.feeModuleType` value reported by markets on collateral fee venues.
pub const FEE_MODULE_TYPE_COLLATERAL: &str = "COLLATERAL";

/// Contract addresses a market trades through.
///
/// Markets on collateral fee venues also report `fee_module` and
/// `fee_module_type`; legacy venues omit both fields.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Venue {
    /// EIP-712 verifying contract for order signing.
    pub exchange: String,
    /// NegRisk adapter, when the market is a NegRisk market.
    #[serde(default)]
    pub adapter: Option<String>,
    /// Fee module that collects the taker fee in USDC on collateral fee venues.
    ///
    /// The signing wallet must approve USDC to this address before placing any
    /// order that is not `post_only`. Always read it from the market.
    #[serde(rename = "feeModule", default, skip_serializing_if = "Option::is_none")]
    pub fee_module: Option<String>,
    /// Fee module type. `"COLLATERAL"` marks a collateral fee venue.
    #[serde(
        rename = "feeModuleType",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub fee_module_type: Option<String>,
}

impl Venue {
    /// Returns `true` when this venue charges the taker fee in USDC through a
    /// separate fee module (`fee_module_type == "COLLATERAL"`).
    ///
    /// Orders on these venues must be signed with `fee_rate_bps = 0`, and
    /// non-post-only orders need a USDC approval to [`Venue::fee_module`].
    pub fn is_collateral_fee_venue(&self) -> bool {
        self.fee_module_type.as_deref() == Some(FEE_MODULE_TYPE_COLLATERAL)
    }

    /// USDC spender required on top of the existing exchange approvals, if any.
    ///
    /// Returns the fee module address on collateral fee venues and `None` on
    /// legacy venues.
    pub fn collateral_fee_approval_target(&self) -> Option<&str> {
        if self.is_collateral_fee_venue() {
            self.fee_module.as_deref().filter(|value| !value.is_empty())
        } else {
            None
        }
    }
}

/// Option-friendly form of [`Venue::is_collateral_fee_venue`]; `None` is a legacy venue.
pub fn is_collateral_fee_venue(venue: Option<&Venue>) -> bool {
    venue.is_some_and(Venue::is_collateral_fee_venue)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MarketTokens {
    pub yes: String,
    pub no: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrderBookEntry {
    pub price: f64,
    pub size: f64,
    pub side: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrderBook {
    pub bids: Vec<OrderBookEntry>,
    pub asks: Vec<OrderBookEntry>,
    #[serde(rename = "tokenId")]
    pub token_id: String,
    #[serde(rename = "adjustedMidpoint")]
    pub adjusted_midpoint: f64,
    #[serde(rename = "maxSpread")]
    pub max_spread: String,
    #[serde(rename = "minSize")]
    pub min_size: String,
    #[serde(rename = "lastTradePrice", default)]
    pub last_trade_price: Option<f64>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Market {
    #[serde(skip, default)]
    pub(crate) client: Option<HttpClient>,

    pub id: i32,
    pub slug: String,
    pub title: String,
    #[serde(rename = "proxyTitle", default)]
    pub proxy_title: Option<String>,
    #[serde(default)]
    pub description: String,
    #[serde(rename = "collateralToken")]
    pub collateral_token: CollateralToken,
    #[serde(rename = "expirationDate")]
    pub expiration_date: String,
    #[serde(rename = "expirationTimestamp")]
    pub expiration_timestamp: i64,
    #[serde(default)]
    pub expired: Option<bool>,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
    pub categories: Vec<String>,
    pub status: String,
    pub creator: MarketCreator,
    pub tags: Vec<String>,
    #[serde(rename = "tradeType")]
    pub trade_type: String,
    #[serde(rename = "marketType")]
    pub market_type: String,
    #[serde(rename = "priorityIndex")]
    pub priority_index: i32,
    pub metadata: MarketMetadata,
    #[serde(default)]
    pub volume: Option<String>,
    #[serde(rename = "volumeFormatted", default)]
    pub volume_formatted: Option<String>,
    #[serde(rename = "automationType", default)]
    pub automation_type: Option<String>,
    #[serde(rename = "imageUrl", default)]
    pub image_url: Option<String>,
    #[serde(default)]
    pub trends: Option<HashMap<String, Value>>,
    #[serde(rename = "openInterest", default)]
    pub open_interest: Option<String>,
    #[serde(rename = "openInterestFormatted", default)]
    pub open_interest_formatted: Option<String>,
    #[serde(default)]
    pub liquidity: Option<String>,
    #[serde(rename = "liquidityFormatted", default)]
    pub liquidity_formatted: Option<String>,
    #[serde(rename = "positionIds", default)]
    pub position_ids: Vec<String>,
    #[serde(rename = "conditionId", default)]
    pub condition_id: Option<String>,
    #[serde(rename = "negRiskRequestId", default)]
    pub neg_risk_request_id: Option<String>,
    #[serde(default)]
    pub tokens: Option<MarketTokens>,
    #[serde(default)]
    pub prices: Vec<f64>,
    #[serde(rename = "tradePrices", default)]
    pub trade_prices: Option<TradePrices>,
    #[serde(rename = "isRewardable", default)]
    pub is_rewardable: Option<bool>,
    #[serde(default)]
    pub settings: Option<MarketSettings>,
    #[serde(default)]
    pub venue: Option<Venue>,
    #[serde(default)]
    pub logo: Option<String>,
    #[serde(rename = "priceOracleMetadata", default)]
    pub price_oracle_data: Option<PriceOracleMetadata>,
    #[serde(rename = "orderInGroup", default)]
    pub order_in_group: Option<i32>,
    #[serde(rename = "winningOutcomeIndex", default)]
    pub winning_outcome_idx: Option<i32>,
    #[serde(rename = "outcomeTokens", default)]
    pub outcome_tokens: Vec<String>,
    #[serde(rename = "ogImageURI", default)]
    pub og_image_uri: Option<String>,
    #[serde(rename = "negRiskMarketId", default)]
    pub neg_risk_market_id: Option<String>,
    #[serde(default)]
    pub markets: Vec<Market>,
    #[serde(rename = "dailyReward", default)]
    pub daily_reward: Option<String>,
    #[serde(default)]
    pub address: Option<String>,
    #[serde(rename = "type", default)]
    pub market_type_legacy: Option<String>,
    #[serde(default)]
    pub outcomes: Vec<MarketOutcome>,
    #[serde(rename = "resolutionDate", default)]
    pub resolution_date: Option<String>,
}

impl std::fmt::Debug for Market {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Market")
            .field("id", &self.id)
            .field("slug", &self.slug)
            .field("title", &self.title)
            .field("status", &self.status)
            .finish()
    }
}

impl Market {
    pub async fn get_user_orders(&self) -> Result<Vec<UserOrder>> {
        let client = self
            .client
            .clone()
            .ok_or_else(|| crate::errors::LimitlessError::invalid_input(
                "this Market instance has no HTTP client attached; fetch it via MarketFetcher first",
            ))?;
        client.require_auth("market_get_user_orders")?;
        client
            .get(&format!(
                "/markets/{}/user-orders",
                urlencoding::encode(&self.slug)
            ))
            .await
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserOrder {
    pub id: String,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "makerAmount")]
    pub maker_amount: Value,
    #[serde(rename = "takerAmount")]
    pub taker_amount: Value,
    #[serde(default)]
    pub expiration: Option<String>,
    #[serde(rename = "signatureType")]
    pub signature_type: i32,
    pub salt: Value,
    pub maker: String,
    pub signer: String,
    pub taker: String,
    #[serde(rename = "tokenId")]
    pub token_id: String,
    pub side: Value,
    #[serde(rename = "feeRateBps")]
    pub fee_rate_bps: i32,
    pub nonce: i32,
    pub signature: String,
    #[serde(rename = "orderType")]
    pub order_type: String,
    #[serde(default)]
    pub price: Option<f64>,
    #[serde(rename = "marketId")]
    pub market_id: i32,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(rename = "filledSize", default)]
    pub filled_size: Option<Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActiveMarketsSortBy {
    #[serde(rename = "lp_rewards")]
    LpRewards,
    #[serde(rename = "ending_soon")]
    EndingSoon,
    #[serde(rename = "newest")]
    Newest,
    #[serde(rename = "high_value")]
    HighValue,
    #[serde(rename = "liquidity")]
    Liquidity,
}

impl AsRef<str> for ActiveMarketsSortBy {
    fn as_ref(&self) -> &str {
        match self {
            Self::LpRewards => "lp_rewards",
            Self::EndingSoon => "ending_soon",
            Self::Newest => "newest",
            Self::HighValue => "high_value",
            Self::Liquidity => "liquidity",
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ActiveMarketsParams {
    pub limit: Option<u32>,
    pub page: Option<u32>,
    pub sort_by: Option<ActiveMarketsSortBy>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ActiveMarketsResponse {
    pub data: Vec<Market>,
    #[serde(rename = "totalMarketsCount")]
    pub total_markets_count: i32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MarketsResponse {
    pub markets: Vec<Market>,
    #[serde(default)]
    pub total: Option<i32>,
    #[serde(default)]
    pub offset: Option<i32>,
    #[serde(default)]
    pub limit: Option<i32>,
}

#[cfg(test)]
mod collateral_fee_venue_tests {
    use super::*;
    use serde_json::json;

    fn collateral_venue() -> Venue {
        serde_json::from_value(json!({
            "exchange": "0x0000000000000000000000000000000000000001",
            "adapter": null,
            "feeModule": "0x0000000000000000000000000000000000000002",
            "feeModuleType": "COLLATERAL"
        }))
        .expect("collateral venue should deserialize")
    }

    fn legacy_venue() -> Venue {
        serde_json::from_value(json!({
            "exchange": "0x0000000000000000000000000000000000000001",
            "adapter": "0x0000000000000000000000000000000000000003"
        }))
        .expect("legacy venue should deserialize")
    }

    #[test]
    fn venue_decodes_fee_module_fields_on_collateral_fee_venues() {
        let venue = collateral_venue();
        assert_eq!(
            venue.fee_module.as_deref(),
            Some("0x0000000000000000000000000000000000000002")
        );
        assert_eq!(
            venue.fee_module_type.as_deref(),
            Some(FEE_MODULE_TYPE_COLLATERAL)
        );
        assert!(venue.is_collateral_fee_venue());
        assert!(is_collateral_fee_venue(Some(&venue)));
        assert_eq!(
            venue.collateral_fee_approval_target(),
            Some("0x0000000000000000000000000000000000000002")
        );
    }

    #[test]
    fn legacy_venue_without_fee_module_fields_is_not_collateral_fee_venue() {
        let venue = legacy_venue();
        assert!(venue.fee_module.is_none());
        assert!(venue.fee_module_type.is_none());
        assert!(!venue.is_collateral_fee_venue());
        assert!(!is_collateral_fee_venue(Some(&venue)));
        assert!(!is_collateral_fee_venue(None));
        assert_eq!(venue.collateral_fee_approval_target(), None);

        let value = serde_json::to_value(&venue).expect("venue should serialize");
        assert!(value.get("feeModule").is_none());
        assert!(value.get("feeModuleType").is_none());
    }

    #[test]
    fn unknown_fee_module_type_is_not_collateral_fee_venue() {
        let mut venue = collateral_venue();
        venue.fee_module_type = Some("OTHER".to_string());
        assert!(!venue.is_collateral_fee_venue());
        assert_eq!(venue.collateral_fee_approval_target(), None);
    }
}
