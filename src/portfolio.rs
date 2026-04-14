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

    pub async fn get_user_history(
        &self,
        page: Option<u32>,
        limit: Option<u32>,
    ) -> Result<HistoryResponse> {
        self.client.require_auth("get_user_history")?;
        let mut query = Serializer::new(String::new());
        query.append_pair("page", &page.unwrap_or(1).to_string());
        query.append_pair("limit", &limit.unwrap_or(10).to_string());
        self.client
            .get(&format!("/portfolio/history?{}", query.finish()))
            .await
    }
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
    #[serde(rename = "latestYesPrice")]
    pub latest_yes_price: f64,
    #[serde(rename = "latestNoPrice")]
    pub latest_no_price: f64,
    #[serde(rename = "outcomeTokenPrice")]
    pub outcome_token_price: f64,
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
pub struct HistoryEntry {
    pub id: String,
    #[serde(rename = "type")]
    pub entry_type: String,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "marketSlug", default)]
    pub market_slug: Option<String>,
    #[serde(default)]
    pub amount: Option<String>,
    #[serde(default)]
    pub details: Option<std::collections::HashMap<String, Value>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HistoryResponse {
    pub data: Vec<HistoryEntry>,
    #[serde(rename = "totalCount")]
    pub total_count: i32,
}
