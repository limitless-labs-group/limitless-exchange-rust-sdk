use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::form_urlencoded::Serializer;

use crate::{errors::Result, http_client::HttpClient};

#[derive(Clone)]
pub struct PortfolioFetcher {
    client: HttpClient,
}

impl PortfolioFetcher {
    pub fn new(client: HttpClient) -> Self {
        Self { client }
    }

    pub async fn get_profile(&self, address: &str) -> Result<UserProfile> {
        self.client.require_auth("get_profile")?;
        self.client
            .get(&format!("/profiles/{}", urlencoding::encode(address)))
            .await
    }

    /// Fetch the authenticated caller's own private profile.
    ///
    /// Resolves the profile from the request credentials (HMAC token or API
    /// key) via `GET /profiles/me`, so no address is required. Use
    /// [`get_profile`](Self::get_profile) when you need a specific account's
    /// profile by address.
    pub async fn get_current_profile(&self) -> Result<UserProfile> {
        self.client.require_auth("get_current_profile")?;
        self.client.get("/profiles/me").await
    }

    pub async fn get_positions(&self) -> Result<PortfolioPositionsResponse> {
        self.client.require_auth("get_positions")?;
        self.client.get("/portfolio/positions").await
    }

    pub async fn get_clob_positions(&self) -> Result<Vec<CLOBPosition>> {
        Ok(self.get_positions().await?.clob)
    }

    pub async fn get_amm_positions(&self) -> Result<Vec<AMMPosition>> {
        Ok(self.get_positions().await?.amm)
    }

    /// Fetch user history with cursor-based pagination.
    ///
    /// `cursor` — pass `None` for the first page (sends `cursor=` empty to
    /// opt into the cursor flow), or `Some("...")` with a previous `nextCursor`.
    /// `limit`  — items per page (1–100, defaults to 20 when omitted).
    pub async fn get_user_history(
        &self,
        cursor: Option<&str>,
        limit: Option<u32>,
    ) -> Result<HistoryResponse> {
        self.client.require_auth("get_user_history")?;
        let url = history_path(cursor, limit);
        self.client.get(&url).await
    }
}

fn history_path(cursor: Option<&str>, limit: Option<u32>) -> String {
    let mut query = Serializer::new(String::new());
    // Always send cursor=, using an empty value on the first page.
    query.append_pair("cursor", cursor.unwrap_or(""));
    query.append_pair("limit", &limit.unwrap_or(20).to_string());
    format!("/portfolio/history?{}", query.finish())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserRank {
    pub id: i32,
    pub name: String,
    #[serde(rename = "feeRateBps")]
    pub fee_rate_bps: i32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReferralData {
    #[serde(rename = "createdAt")]
    pub created_at: String,
    pub id: i32,
    #[serde(rename = "referredProfileId")]
    pub referred_profile_id: i32,
    #[serde(rename = "pfpUrl", default)]
    pub pfp_url: Option<String>,
    #[serde(rename = "displayName")]
    pub display_name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserProfile {
    pub id: i32,
    pub account: String,
    #[serde(default)]
    pub rank: Option<UserRank>,
    #[serde(rename = "createdAt", default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(rename = "displayName", default)]
    pub display_name: Option<String>,
    #[serde(rename = "pfpUrl", default)]
    pub pfp_url: Option<String>,
    #[serde(default)]
    pub bio: Option<String>,
    #[serde(rename = "socialUrl", default)]
    pub social_url: Option<String>,
    #[serde(rename = "tradeWalletOption", default)]
    pub trade_wallet_option: Option<String>,
    #[serde(rename = "embeddedAccount", default)]
    pub embedded_account: Option<String>,
    #[serde(default)]
    pub points: Option<f64>,
    #[serde(rename = "accumulativePoints", default)]
    pub accumulative_points: Option<f64>,
    #[serde(rename = "enrolledInPointsProgram", default)]
    pub enrolled_in_points_program: Option<bool>,
    #[serde(rename = "leaderboardPosition", default)]
    pub leaderboard_position: Option<i32>,
    #[serde(rename = "isTop100", default)]
    pub is_top_100: Option<bool>,
    #[serde(rename = "isCaptain", default)]
    pub is_captain: Option<bool>,
    #[serde(rename = "referralData", default)]
    pub referral_data: Vec<ReferralData>,
    #[serde(rename = "referredUsersCount", default)]
    pub referred_users_count: Option<i32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PositionMarketGroup {
    #[serde(default)]
    pub slug: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PositionMarket {
    pub id: Value,
    pub slug: String,
    pub title: String,
    #[serde(default)]
    pub status: Option<String>,
    pub closed: bool,
    pub deadline: String,
    #[serde(rename = "conditionId", default)]
    pub condition_id: Option<String>,
    #[serde(rename = "winningOutcomeIndex", default)]
    pub winning_outcome_index: Option<i32>,
    #[serde(default)]
    pub group: Option<PositionMarketGroup>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PositionSide {
    pub cost: String,
    #[serde(rename = "fillPrice")]
    pub fill_price: String,
    #[serde(rename = "marketValue")]
    pub market_value: String,
    #[serde(rename = "realisedPnl")]
    pub realised_pnl: String,
    #[serde(rename = "unrealizedPnl")]
    pub unrealized_pnl: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TokenBalance {
    pub yes: String,
    pub no: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LatestTrade {
    #[serde(rename = "latestYesPrice", default)]
    pub latest_yes_price: Option<f64>,
    #[serde(rename = "latestNoPrice", default)]
    pub latest_no_price: Option<f64>,
    #[serde(rename = "outcomeTokenPrice", default)]
    pub outcome_token_price: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClobPositionOrders {
    #[serde(rename = "liveOrders")]
    pub live_orders: Vec<Value>,
    #[serde(rename = "totalCollateralLocked")]
    pub total_collateral_locked: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClobPositionRewards {
    pub epochs: Vec<Value>,
    #[serde(rename = "isEarning")]
    pub is_earning: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClobPositionSides {
    pub yes: PositionSide,
    pub no: PositionSide,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CLOBPosition {
    pub market: PositionMarket,
    #[serde(rename = "makerAddress")]
    pub maker_address: String,
    pub positions: ClobPositionSides,
    #[serde(rename = "tokensBalance")]
    pub tokens_balance: TokenBalance,
    #[serde(rename = "latestTrade")]
    pub latest_trade: LatestTrade,
    #[serde(default)]
    pub orders: Option<ClobPositionOrders>,
    #[serde(default)]
    pub rewards: Option<ClobPositionRewards>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AmmLatestTrade {
    #[serde(rename = "outcomeTokenPrice")]
    pub outcome_token_price: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AMMPosition {
    pub market: PositionMarket,
    pub account: String,
    #[serde(rename = "outcomeIndex")]
    pub outcome_index: i32,
    #[serde(rename = "collateralAmount")]
    pub collateral_amount: String,
    #[serde(rename = "outcomeTokenAmount")]
    pub outcome_token_amount: String,
    #[serde(rename = "averageFillPrice")]
    pub average_fill_price: String,
    #[serde(rename = "totalBuysCost")]
    pub total_buys_cost: String,
    #[serde(rename = "totalSellsCost")]
    pub total_sells_cost: String,
    #[serde(rename = "realizedPnl")]
    pub realized_pnl: String,
    #[serde(rename = "unrealizedPnl")]
    pub unrealized_pnl: String,
    #[serde(rename = "latestTrade", default)]
    pub latest_trade: Option<AmmLatestTrade>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PortfolioRewards {
    #[serde(rename = "todaysRewards")]
    pub todays_rewards: String,
    #[serde(rename = "rewardsByEpoch")]
    pub rewards_by_epoch: Vec<Value>,
    #[serde(rename = "rewardsChartData")]
    pub rewards_chart_data: Vec<Value>,
    #[serde(rename = "totalUnpaidRewards")]
    pub total_unpaid_rewards: String,
    #[serde(rename = "totalUserRewardsLastEpoch")]
    pub total_user_rewards_last_epoch: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PortfolioPositionsResponse {
    #[serde(default)]
    pub amm: Vec<AMMPosition>,
    #[serde(default)]
    pub clob: Vec<CLOBPosition>,
    #[serde(default)]
    pub group: Vec<Value>,
    #[serde(default)]
    pub points: Option<String>,
    #[serde(rename = "accumulativePoints", default)]
    pub accumulative_points: Option<String>,
    #[serde(default)]
    pub rewards: Option<PortfolioRewards>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Position {
    #[serde(rename = "type")]
    pub position_type: String,
    pub market: PositionMarket,
    pub side: String,
    #[serde(rename = "costBasis")]
    pub cost_basis: f64,
    #[serde(rename = "marketValue")]
    pub market_value: f64,
    #[serde(rename = "unrealizedPnl")]
    pub unrealized_pnl: f64,
    #[serde(rename = "realizedPnl")]
    pub realized_pnl: f64,
    #[serde(rename = "currentPrice")]
    pub current_price: f64,
    #[serde(rename = "avgPrice")]
    pub avg_price: f64,
    #[serde(rename = "tokenBalance")]
    pub token_balance: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PortfolioBreakdownEntry {
    pub positions: i32,
    pub value: f64,
    pub pnl: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PortfolioBreakdown {
    pub clob: PortfolioBreakdownEntry,
    pub amm: PortfolioBreakdownEntry,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PortfolioSummary {
    #[serde(rename = "totalValue")]
    pub total_value: f64,
    #[serde(rename = "totalCostBasis")]
    pub total_cost_basis: f64,
    #[serde(rename = "totalUnrealizedPnl")]
    pub total_unrealized_pnl: f64,
    #[serde(rename = "totalRealizedPnl")]
    pub total_realized_pnl: f64,
    #[serde(rename = "totalUnrealizedPnlPercent")]
    pub total_unrealized_pnl_percent: f64,
    #[serde(rename = "positionCount")]
    pub position_count: i32,
    #[serde(rename = "marketCount")]
    pub market_count: i32,
    pub breakdown: PortfolioBreakdown,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HistoryMarketCollateral {
    pub symbol: String,
    pub id: String,
    pub decimals: i32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HistoryMarket {
    pub closed: bool,
    #[serde(default)]
    pub collateral: Option<HistoryMarketCollateral>,
    #[serde(default)]
    pub group: Option<Value>,
    #[serde(rename = "conditionId", default)]
    pub condition_id: Option<String>,
    #[serde(default)]
    pub funding: Option<String>,
    pub id: String,
    pub slug: String,
    pub title: String,
    #[serde(rename = "expirationDate", default)]
    pub expiration_date: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HistoryEntry {
    #[serde(rename = "blockTimestamp")]
    pub block_timestamp: i64,
    #[serde(rename = "collateralAmount", default)]
    pub collateral_amount: Option<String>,
    #[serde(default)]
    pub market: Option<HistoryMarket>,
    #[serde(rename = "outcomeIndex", default)]
    pub outcome_index: Option<i32>,
    #[serde(rename = "outcomeTokenAmount", default)]
    pub outcome_token_amount: Option<String>,
    #[serde(rename = "outcomeTokenAmounts", default)]
    pub outcome_token_amounts: Option<Vec<String>>,
    #[serde(rename = "outcomeTokenPrice", default)]
    pub outcome_token_price: Option<Value>,
    #[serde(default)]
    pub strategy: Option<String>,
    #[serde(rename = "transactionHash", default)]
    pub transaction_hash: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HistoryResponse {
    pub data: Vec<HistoryEntry>,
    #[serde(rename = "nextCursor")]
    pub next_cursor: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn history_path_uses_empty_cursor_and_default_limit_on_first_page() {
        assert_eq!(
            history_path(None, None),
            "/portfolio/history?cursor=&limit=20"
        );
    }

    #[test]
    fn history_path_forwards_cursor_and_limit() {
        assert_eq!(
            history_path(Some("cursor-1"), Some(5)),
            "/portfolio/history?cursor=cursor-1&limit=5"
        );
    }

    #[test]
    fn history_response_deserializes_cursor_shape() {
        let response: HistoryResponse = serde_json::from_value(json!({
            "data": [{
                "blockTimestamp": 1712345678,
                "collateralAmount": "15.25",
                "market": {
                    "closed": false,
                    "collateral": {
                        "symbol": "USDC",
                        "id": "usdc",
                        "decimals": 6
                    },
                    "conditionId": "0xcond",
                    "funding": "1000",
                    "id": "market-1",
                    "slug": "btc-above-100k",
                    "title": "BTC above 100k?",
                    "expirationDate": "2026-12-31T00:00:00.000Z"
                },
                "outcomeIndex": 0,
                "outcomeTokenAmount": "20",
                "outcomeTokenAmounts": ["20", "0"],
                "outcomeTokenPrice": 0.76,
                "strategy": "Buy",
                "transactionHash": "0xtx1"
            }],
            "nextCursor": "cursor-2"
        }))
        .expect("history response should deserialize");

        assert_eq!(response.next_cursor.as_deref(), Some("cursor-2"));
        assert_eq!(response.data.len(), 1);

        let entry = &response.data[0];
        assert_eq!(entry.block_timestamp, 1_712_345_678);
        assert_eq!(entry.strategy.as_deref(), Some("Buy"));
        assert_eq!(entry.transaction_hash.as_deref(), Some("0xtx1"));
        assert_eq!(
            entry.market.as_ref().map(|m| m.slug.as_str()),
            Some("btc-above-100k")
        );
        assert_eq!(entry.outcome_token_price, Some(json!(0.76)));
    }

    #[test]
    fn clob_position_latest_trade_deserializes_without_price_fields() {
        let position: CLOBPosition = serde_json::from_value(json!({
            "market": {
                "id": 1,
                "slug": "btc-above-100k",
                "title": "BTC above 100k?",
                "closed": false,
                "deadline": "2026-12-31T00:00:00.000Z"
            },
            "makerAddress": "0xmaker",
            "positions": {
                "yes": {
                    "cost": "10",
                    "fillPrice": "0.5",
                    "marketValue": "12",
                    "realisedPnl": "0",
                    "unrealizedPnl": "2"
                },
                "no": {
                    "cost": "0",
                    "fillPrice": "0",
                    "marketValue": "0",
                    "realisedPnl": "0",
                    "unrealizedPnl": "0"
                }
            },
            "tokensBalance": {
                "yes": "20",
                "no": "0"
            },
            "latestTrade": {}
        }))
        .expect("clob position should deserialize");

        assert_eq!(position.latest_trade.latest_yes_price, None);
        assert_eq!(position.latest_trade.latest_no_price, None);
        assert_eq!(position.latest_trade.outcome_token_price, None);
    }
}
