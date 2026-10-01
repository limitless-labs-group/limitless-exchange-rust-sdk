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

/// Public market metadata.
///
/// `fee` is `null` or absent on markets without a fee flag. Every other wire
/// key (sports and venue fields such as `externalSlug`, `homeTeam`,
/// `sportType`, `startMatchTimestampInUTC`) is preserved verbatim in `extra`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MarketMetadata {
    #[serde(default)]
    pub fee: Option<bool>,
    #[serde(rename = "isBannered", default)]
    pub is_bannered: Option<bool>,
    #[serde(rename = "isPolyArbitrage", default)]
    pub is_poly_arbitrage: Option<bool>,
    #[serde(rename = "shouldMarketMake", default)]
    pub should_market_make: Option<bool>,
    #[serde(rename = "openPrice", default)]
    pub open_price: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
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
    /// Per-market taker delay in milliseconds. Non-zero means taker orders are
    /// accepted with `settlementStatus: "DELAYED"` and released after the delay.
    #[serde(rename = "takerDelayMs", default)]
    pub taker_delay_ms: Option<i64>,
    #[serde(rename = "creatorFeePct", default)]
    pub creator_fee_pct: Option<f64>,
    #[serde(rename = "effectiveDailyReward", default)]
    pub effective_daily_reward: Option<String>,
    #[serde(rename = "currentRewardsMultiplier", default)]
    pub current_rewards_multiplier: Option<f64>,
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
    /// `null` on markets whose chart is sourced from Binance or Pyth Pro
    /// (hourly and daily up-or-down replicas); set for Pyth and Chainlink feeds.
    #[serde(rename = "pythAddress", default)]
    pub pyth_address: Option<String>,
    /// `chainlink`, `pyth`, `pyth-pro` or `binance`.
    #[serde(rename = "chartSource", default)]
    pub chart_source: Option<String>,
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
    /// Raw midpoint before the `adjustedMidpoint` size filter is applied.
    #[serde(default)]
    pub midpoint: Option<f64>,
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

/// One row of `GET /markets/{slug}/user-orders`.
///
/// This is the matching-engine order record, not the signed order: amounts and
/// sizes are raw 6-decimal units serialized as decimal strings, `price` is a
/// decimal string such as `"0.65"`, and `market_id` is the internal market id
/// serialized as text.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserOrder {
    pub id: String,
    pub owner_id: i32,
    pub market_id: String,
    pub token: String,
    /// `GTC`, `FAK` or `FOK`.
    #[serde(rename = "type")]
    pub order_type: String,
    /// `LIVE`, `MATCHED`, `CANCELED` or `UNMATCHED`.
    pub status: String,
    /// `BUY` or `SELL`.
    pub side: String,
    pub maker_amount: String,
    pub taker_amount: String,
    pub price: String,
    pub original_size: String,
    pub remaining_size: String,
    pub created_at: String,
    #[serde(default)]
    pub client_order_id: Option<String>,
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
    #[serde(rename = "trending")]
    Trending,
}

impl AsRef<str> for ActiveMarketsSortBy {
    fn as_ref(&self) -> &str {
        match self {
            Self::LpRewards => "lp_rewards",
            Self::EndingSoon => "ending_soon",
            Self::Newest => "newest",
            Self::HighValue => "high_value",
            Self::Trending => "trending",
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
mod tests {
    use super::*;

    /// Verbatim `priceOracleMetadata` from a live `GET /markets/active` row
    /// (an hourly up-or-down replica charted from Binance).
    const ORACLE_BINANCE: &str = r#"{"ticker":"ETHUSDT","assetType":"CRYPTO","pythAddress":null,"chartSource":"binance","chainlinkFeedId":null,"chainlinkFeedAddress":null,"chainlinkPair":null,"chainlinkStreamUrl":null,"symbol":"ETH","name":"ETH"}"#;

    /// Verbatim `settings` from the same row.
    const SETTINGS_LIVE: &str = r#"{"minSize":"100000000","maxSpread":0.035,"dailyReward":"0","effectiveDailyReward":"0","currentRewardsMultiplier":1,"rewardsEpoch":"0","c":"3","rebateRate":"1","takerDelayMs":500,"creatorFeePct":0}"#;

    /// Verbatim `metadata` from a live sports (tennis) market row.
    const METADATA_SPORTS: &str = r#"{"fee":true,"round":"ATP Tokyo - 1/16-finals","eventId":"18769795","awayTeam":"Arthur Fery","homeTeam":"Jaime Faria","eventSlug":"jaime-faria-arthur-fery","player1Id":"239511","player2Id":"146848","sportType":"tennis","roundShort":"1/16","seriesSlug":"atp","player1Name":"Jaime Faria","player1Slug":"jaime-faria","player2Name":"Arthur Fery","player2Slug":"arthur-fery","externalSlug":"atp-faria-fery-2026-09-30","leagueNameFull":"ATP Japan Open","player1Country":"Portugal","player2Country":"United Kingdom","tournamentName":"ATP Japan Open","daysBeforeMatch":3,"player1PhotoUrl":"https://api.api-tennis.com/logo-tennis/824_j-faria.jpg","player2PhotoUrl":"https://api.api-tennis.com/logo-tennis/8174_a-fery.jpg","binaryMarketType":"moneyline","player1ShortName":"J. Faria","player2ShortName":"A. Fery","apiTennisEventKey":12167327,"apiTennisPlayer1Key":824,"apiTennisPlayer2Key":8174,"startMatchTimestampInUTC":1790820000}"#;

    /// Verbatim `GET /markets/{slug}/orderbook` response.
    const ORDERBOOK_LIVE: &str = r#"{"bids":[{"price":0.99,"size":1000000000,"side":"BUY"},{"price":0.001,"size":1000000000,"side":"BUY"}],"asks":[],"tokenId":"74222001790536374571213216822902367122230225247267225320124669788760337719428","adjustedMidpoint":0.995,"midpoint":0.995,"maxSpread":"0.035","minSize":"100000000","lastTradePrice":0.999}"#;

    /// Verbatim market row from the default `GET /markets/active?limit=25` page.
    /// Before the `pythAddress` fix this single row failed the whole page.
    const MARKET_ROW_PYTH_NULL: &str = r#"{"id":415505,"automationType":"lumy","conditionId":"0xa59e4a82039e90725adfb0ba820b48e2e025ff5de9e80653b499332d8d26c377","negRiskRequestId":null,"description":"<p>This market will resolve to &quot;Up&quot; if the close price is greater than or equal to the open price for the ETH/USDT 1 hour candle that begins on the time and date specified in the title. Otherwise, this market will resolve to &quot;Down&quot;.</p><p><br></p><p>The resolution source for this market is information from Binance, specifically the ETH/USDT pair (<a href=\"https://www.binance.com/en/trade/ETH_USDT\" target=\"_blank\" rel=\"noopener noreferrer\">https://www.binance.com/en/trade/ETH_USDT</a>). The close \u00ab C \u00bb and open \u00ab O \u00bb displayed at the top of the graph for the relevant &quot;1H&quot; candle will be used once the data for that candle is finalized.</p><p><br></p><p>Please note that this market is about the price according to Binance ETH/USDT, not according to other exchanges or trading pairs.</p>","groupId":null,"collateralToken":{"address":"0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913","decimals":6,"symbol":"USDC"},"title":"ETH Up or Down Hourly","proxyTitle":null,"expirationDate":"Sep 30, 2026","expirationTimestamp":1790798400000,"createdAt":"2026-09-30T19:00:10.246Z","startAt":"2026-09-30T19:00:00.000Z","updatedAt":"2026-09-30T19:00:10.247Z","categories":["Crypto","Hourly"],"status":"FUNDED","expired":false,"hidden":false,"creator":{"name":"Limitless","username":"Limitless","imageURI":"https://limitless.exchange/assets/images/logo.svg","link":"https://x.com/trylimitless","address":"0x60e61631d9a585797cDee9f79FafDecf3bd7F64D"},"tags":["Lumy","Pareto"],"volume":"19494137","volumeFormatted":"19.494137","tokens":{"yes":"44202356283824256304586192704200864982320010473849163919479338814419352939973","no":"111996256696538381693272919455643134730466436679166847614120087332983140081510"},"prices":[0.209,0.791],"tradePrices":{"buy":{"market":[0.223,0.805],"limit":[0.195,0.777]},"sell":{"market":[0.195,0.777],"limit":[0.223,0.805]}},"isOther":false,"isRewardable":true,"slug":"eth-up-or-down-hourly-p-1790784310382","stableSlug":"eth-hourly-price","tradeType":"clob","venue":{"exchange":"0x05c748E2f4DcDe0ec9Fa8DDc40DE6b867f923fa5","adapter":null},"marketType":"single","priorityIndex":0,"properties":[{"propertyKeySlug":"domain","value":["crypto"]},{"propertyKeySlug":"duration","value":["hourly"]},{"propertyKeySlug":"ticker","value":["eth"]}],"winningOutcomeIndex":null,"metadata":{"fee":true,"chart":{"candle":"hourly","market":"spot","source":"binance","symbol":"ETHUSDT","windowOpenAt":"2026-09-30T19:00:00.000Z"},"openPrice":"2679.16","isBannered":false,"externalSlug":"ethereum-up-or-down-september-30-2026-3pm-et","isPolyArbitrage":true,"externalProvider":"polymarket","polymarketSeriesId":10117,"openPriceCapturedAt":"2026-09-30T19:00:00.000Z"},"settings":{"minSize":"100000000","maxSpread":0.035,"dailyReward":"0","effectiveDailyReward":"0","currentRewardsMultiplier":1,"rewardsEpoch":"0","c":"3","rebateRate":"1","takerDelayMs":500,"creatorFeePct":0},"imageUrl":"https://cdn.limitless.exchange/price-oracles/58/d570277d-5f47-4206-a69d-44e7b4e8570d.svg","logo":"https://cdn.limitless.exchange/price-oracles/58/d570277d-5f47-4206-a69d-44e7b4e8570d.svg","priceOracleMetadata":{"ticker":"ETHUSDT","assetType":"CRYPTO","pythAddress":null,"chartSource":"binance","chainlinkFeedId":null,"chainlinkFeedAddress":null,"chainlinkPair":null,"chainlinkStreamUrl":null,"symbol":"ETH","name":"ETH"}}"#;

    #[test]
    fn price_oracle_metadata_accepts_null_pyth_address() {
        let oracle: PriceOracleMetadata =
            serde_json::from_str(ORACLE_BINANCE).expect("binance replica should decode");
        assert_eq!(oracle.pyth_address, None);
        assert_eq!(oracle.chart_source.as_deref(), Some("binance"));
        assert_eq!(oracle.ticker, "ETHUSDT");
    }

    #[test]
    fn active_market_row_with_null_pyth_address_decodes() {
        let market: Market =
            serde_json::from_str(MARKET_ROW_PYTH_NULL).expect("live market row should decode");
        assert_eq!(market.slug, "eth-up-or-down-hourly-p-1790784310382");
        let oracle = market.price_oracle_data.expect("oracle metadata present");
        assert_eq!(oracle.pyth_address, None);
        let settings = market.settings.expect("settings present");
        assert_eq!(settings.taker_delay_ms, Some(500));
        assert_eq!(market.metadata.fee, Some(true));
    }

    #[test]
    fn market_settings_keep_taker_delay_and_reward_fields() {
        let settings: MarketSettings =
            serde_json::from_str(SETTINGS_LIVE).expect("settings should decode");
        assert_eq!(settings.min_size, "100000000");
        assert_eq!(settings.taker_delay_ms, Some(500));
        assert_eq!(settings.creator_fee_pct, Some(0.0));
        assert_eq!(settings.effective_daily_reward.as_deref(), Some("0"));
        assert_eq!(settings.current_rewards_multiplier, Some(1.0));
    }

    #[test]
    fn market_metadata_tolerates_null_fee_and_keeps_extra_keys() {
        let empty: MarketMetadata = serde_json::from_str("{}").expect("null metadata is `{}`");
        assert_eq!(empty.fee, None);
        assert!(empty.extra.is_empty());

        let null_fee: MarketMetadata =
            serde_json::from_str(r#"{"fee":null}"#).expect("fee may be null");
        assert_eq!(null_fee.fee, None);

        let sports: MarketMetadata =
            serde_json::from_str(METADATA_SPORTS).expect("sports metadata should decode");
        assert_eq!(sports.fee, Some(true));
        assert_eq!(sports.extra["sportType"], "tennis");
        assert_eq!(sports.extra["externalSlug"], "atp-faria-fery-2026-09-30");
        assert_eq!(sports.extra["startMatchTimestampInUTC"], 1_790_820_000);
        assert!(!sports.extra.contains_key("fee"));
    }

    #[test]
    fn order_book_exposes_midpoint_alongside_adjusted_midpoint() {
        let book: OrderBook =
            serde_json::from_str(ORDERBOOK_LIVE).expect("orderbook should decode");
        assert_eq!(book.midpoint, Some(0.995));
        assert_eq!(book.adjusted_midpoint, 0.995);
        assert_eq!(book.max_spread, "0.035");
        assert_eq!(book.min_size, "100000000");
        assert_eq!(book.last_trade_price, Some(0.999));
        assert_eq!(book.bids.len(), 2);
        assert!(book.asks.is_empty());
    }

    #[test]
    fn user_orders_decode_the_ome_row_shape() {
        // Shape per the API `UserOrder` DTO: numeric values are decimal strings,
        // `marketId` is text, `clientOrderId` is only present when set.
        let body = r#"[
            {
                "id": "7c1a9f1e-5b7a-4c0e-9d7e-2f0b1a6c3d44",
                "createdAt": "2026-09-30T19:05:12.345Z",
                "ownerId": 12345,
                "marketId": "9876",
                "token": "74222001790536374571213216822902367122230225247267225320124669788760337719428",
                "type": "GTC",
                "status": "LIVE",
                "side": "BUY",
                "makerAmount": "6500000",
                "takerAmount": "10000000",
                "price": "0.65",
                "originalSize": "10000000",
                "remainingSize": "10000000",
                "clientOrderId": "my-tag-42"
            },
            {
                "id": "0b2d6c7a-1e3f-4a5b-8c9d-0e1f2a3b4c5d",
                "createdAt": "2026-09-30T19:06:00.000Z",
                "ownerId": 12345,
                "marketId": "9876",
                "token": "74222001790536374571213216822902367122230225247267225320124669788760337719428",
                "type": "FAK",
                "status": "MATCHED",
                "side": "SELL",
                "makerAmount": "10000000",
                "takerAmount": "3500000",
                "price": "0.35",
                "originalSize": "10000000",
                "remainingSize": "0"
            }
        ]"#;
        let orders: Vec<UserOrder> = serde_json::from_str(body).expect("user orders should decode");
        assert_eq!(orders.len(), 2);
        assert_eq!(orders[0].market_id, "9876");
        assert_eq!(orders[0].order_type, "GTC");
        assert_eq!(orders[0].status, "LIVE");
        assert_eq!(orders[0].side, "BUY");
        assert_eq!(orders[0].price, "0.65");
        assert_eq!(orders[0].remaining_size, "10000000");
        assert_eq!(orders[0].client_order_id.as_deref(), Some("my-tag-42"));
        assert_eq!(orders[1].client_order_id, None);
        assert_eq!(orders[1].remaining_size, "0");

        let empty: Vec<UserOrder> = serde_json::from_str("[]").expect("empty list decodes");
        assert!(empty.is_empty());
    }

    #[test]
    fn active_markets_sort_options_match_the_server_enum() {
        let variants = [
            ActiveMarketsSortBy::EndingSoon,
            ActiveMarketsSortBy::HighValue,
            ActiveMarketsSortBy::LpRewards,
            ActiveMarketsSortBy::Newest,
            ActiveMarketsSortBy::Trending,
        ];
        let wire: Vec<&str> = variants.iter().map(|v| v.as_ref()).collect();
        // MarketSortOption on the API: ending_soon, high_value, lp_rewards, newest, trending.
        assert_eq!(
            wire,
            [
                "ending_soon",
                "high_value",
                "lp_rewards",
                "newest",
                "trending"
            ]
        );
        for variant in &variants {
            assert_eq!(
                serde_json::to_value(variant).expect("serializes"),
                serde_json::Value::String(variant.as_ref().to_string())
            );
        }
        assert_eq!(
            active_markets_endpoint(Some(&ActiveMarketsParams {
                limit: Some(10),
                page: Some(1),
                sort_by: Some(ActiveMarketsSortBy::Trending),
            })),
            "/markets/active?limit=10&page=1&sortBy=trending"
        );
    }
}
