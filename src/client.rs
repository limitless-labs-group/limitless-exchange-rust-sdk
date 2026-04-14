use crate::{
    api_tokens::ApiTokenService,
    delegated_orders::DelegatedOrderService,
    errors::Result,
    http_client::{HttpClient, HttpClientBuilder},
    market_pages::MarketPageFetcher,
    markets::MarketFetcher,
    orders::{OrderClient, OrderClientOptions},
    partner_accounts::PartnerAccountService,
    portfolio::PortfolioFetcher,
    server_wallets::ServerWalletService,
    websocket::{WebSocketClient, WebSocketConfig},
};

pub struct Client {
    pub http: HttpClient,
    pub markets: MarketFetcher,
    pub portfolio: PortfolioFetcher,
    pub pages: MarketPageFetcher,
    pub api_tokens: ApiTokenService,
    pub partner_accounts: PartnerAccountService,
    pub delegated_orders: DelegatedOrderService,
    pub server_wallets: ServerWalletService,
}

impl Client {
    pub fn builder() -> HttpClientBuilder {
        HttpClient::builder()
    }

    pub fn new() -> Result<Self> {
        Self::from_http_client(HttpClient::builder().build()?)
    }

    pub fn from_http_client(http: HttpClient) -> Result<Self> {
        Ok(Self {
            markets: MarketFetcher::new(http.clone()),
            portfolio: PortfolioFetcher::new(http.clone()),
            pages: MarketPageFetcher::new(http.clone()),
            api_tokens: ApiTokenService::new(http.clone()),
            partner_accounts: PartnerAccountService::new(http.clone()),
            delegated_orders: DelegatedOrderService::new(http.clone(), Some(http.logger())),
            server_wallets: ServerWalletService::new(http.clone()),
            http,
        })
    }

    pub fn new_order_client(
        &self,
        private_key_hex: &str,
        options: Option<OrderClientOptions>,
    ) -> Result<OrderClient> {
        let mut merged = options.unwrap_or_default();
        if merged.market_fetcher.is_none() {
            merged.market_fetcher = Some(self.markets.clone());
        }
        if merged.portfolio_fetcher.is_none() {
            merged.portfolio_fetcher = Some(self.portfolio.clone());
        }
        if merged.logger.is_none() {
            merged.logger = Some(self.http.logger());
        }

        OrderClient::new(self.http.clone(), private_key_hex, Some(merged))
    }

    pub fn new_websocket_client(&self, config: Option<WebSocketConfig>) -> WebSocketClient {
        let mut config = config.unwrap_or_default();
        if config.api_key.is_none() {
            config.api_key = self.http.api_key();
        }
        if config.hmac_credentials.is_none() {
            config.hmac_credentials = self.http.hmac_credentials();
        }
        if config.logger.is_none() {
            config.logger = Some(self.http.logger());
        }

        WebSocketClient::new(Some(config))
    }
}
