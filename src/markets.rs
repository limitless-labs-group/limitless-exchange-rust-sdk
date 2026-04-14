use std::{collections::HashMap, sync::RwLock};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::form_urlencoded::Serializer;

use crate::{errors::Result, http_client::HttpClient};

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

        let mut response: ActiveMarketsResponse = self.client.get(&endpoint).await?;
        for market in &mut response.data {
            market.client = Some(self.client.clone());
        }
        Ok(response)
    }

    pub async fn get_market(&self, slug: &str) -> Result<Market> {
        let mut market: Market = self
            .client
            .get(&format!("/markets/{}", urlencoding::encode(slug)))
            .await?;
        market.client = Some(self.client.clone());

        if let Some(venue) = market.venue.clone() {
            self.venue_cache
                .write()
                .unwrap_or_else(|err| err.into_inner())
                .insert(slug.to_string(), venue);
        }
        Ok(market)
    }

    pub fn get_venue(&self, slug: &str) -> Option<Venue> {
        self.venue_cache
            .read()
            .unwrap_or_else(|err| err.into_inner())
            .get(slug)
            .cloned()
    }

    pub async fn get_order_book(&self, slug: &str) -> Result<OrderBook> {
        self.client
            .get(&format!("/markets/{}/orderbook", urlencoding::encode(slug)))
            .await
    }

    pub async fn get_user_orders(&self, slug: &str) -> Result<Vec<UserOrder>> {
        self.client.require_auth("get_user_orders")?;
        self.client
            .get(&format!(
                "/markets/{}/user-orders",
                urlencoding::encode(slug)
            ))
            .await
    }
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
    pub logo: String,
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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Venue {
    pub exchange: String,
    #[serde(default)]
    pub adapter: Option<String>,
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
    #[serde(rename = "lastTradePrice")]
    pub last_trade_price: f64,
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
