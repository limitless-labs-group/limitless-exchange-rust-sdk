use std::{
    collections::{BTreeMap, HashMap},
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex, RwLock,
    },
    time::Duration,
};

use futures_util::{sink::SinkExt, stream::SplitSink, stream::SplitStream, StreamExt};
use http::{HeaderMap, HeaderValue, Request};
use serde::{de::Error as DeError, Deserialize, Deserializer, Serialize};
use serde_json::Value;
use tokio::{net::TcpStream, sync::oneshot, time::sleep};
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};

use crate::{
    constants::DEFAULT_WS_URL,
    errors::{LimitlessError, Result},
    hmac::{compute_hmac_signature, HmacCredentials},
    logger::{noop_logger, SharedLogger},
    time_utils::chrono_timestamp,
};

const SDK_ID: &str = "lmts-sdk-rs";
const SOCKET_IO_PATH: &str = "/socket.io/?EIO=4&transport=websocket";
const SOCKET_NAMESPACE: &str = "/markets";

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;
type WsWrite = SplitSink<WsStream, Message>;
type WsRead = SplitStream<WsStream>;
type EventHandler = Arc<dyn Fn(Value) + Send + Sync>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WebSocketState {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SubscriptionChannel {
    Orderbook,
    Trades,
    Orders,
    Fills,
    Markets,
    Prices,
    SubscribeMarketPrices,
    SubscribePositions,
    SubscribeTransactions,
}

impl SubscriptionChannel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Orderbook => "orderbook",
            Self::Trades => "trades",
            Self::Orders => "orders",
            Self::Fills => "fills",
            Self::Markets => "markets",
            Self::Prices => "prices",
            Self::SubscribeMarketPrices => "subscribe_market_prices",
            Self::SubscribePositions => "subscribe_positions",
            Self::SubscribeTransactions => "subscribe_transactions",
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct SubscriptionOptions {
    #[serde(rename = "marketSlug", skip_serializing_if = "Option::is_none")]
    pub market_slug: Option<String>,
    #[serde(rename = "marketSlugs", skip_serializing_if = "Vec::is_empty", default)]
    pub market_slugs: Vec<String>,
    #[serde(rename = "marketAddress", skip_serializing_if = "Option::is_none")]
    pub market_address: Option<String>,
    #[serde(
        rename = "marketAddresses",
        skip_serializing_if = "Vec::is_empty",
        default
    )]
    pub market_addresses: Vec<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub filters: BTreeMap<String, Value>,
}

#[derive(Clone)]
pub struct WebSocketConfig {
    pub url: String,
    pub api_key: Option<String>,
    pub hmac_credentials: Option<HmacCredentials>,
    pub auto_reconnect: bool,
    pub reconnect_delay_ms: u64,
    pub max_reconnect_attempts: u32,
    pub timeout_ms: u64,
    pub logger: Option<SharedLogger>,
}

impl Default for WebSocketConfig {
    fn default() -> Self {
        Self {
            url: DEFAULT_WS_URL.to_string(),
            api_key: std::env::var("LIMITLESS_API_KEY").ok(),
            hmac_credentials: None,
            auto_reconnect: true,
            reconnect_delay_ms: 1_000,
            max_reconnect_attempts: 0,
            timeout_ms: 10_000,
            logger: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlexFloat(pub f64);

impl FlexFloat {
    pub fn float64(self) -> f64 {
        self.0
    }
}

impl<'de> Deserialize<'de> for FlexFloat {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match Value::deserialize(deserializer)? {
            Value::Number(number) => number
                .as_f64()
                .map(Self)
                .ok_or_else(|| D::Error::custom("expected f64-compatible number")),
            Value::String(value) => value
                .parse::<f64>()
                .map(Self)
                .map_err(|err| D::Error::custom(format!("cannot parse float {value}: {err}"))),
            other => Err(D::Error::custom(format!(
                "cannot deserialize float from {other}"
            ))),
        }
    }
}

impl Serialize for FlexFloat {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_f64(self.0)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrderbookData {
    pub bids: Vec<crate::markets::OrderBookEntry>,
    pub asks: Vec<crate::markets::OrderBookEntry>,
    #[serde(rename = "tokenId")]
    pub token_id: String,
    #[serde(rename = "adjustedMidpoint")]
    pub adjusted_midpoint: f64,
    #[serde(rename = "maxSpread")]
    pub max_spread: FlexFloat,
    #[serde(rename = "minSize")]
    pub min_size: FlexFloat,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrderbookUpdate {
    #[serde(rename = "marketSlug")]
    pub market_slug: String,
    pub orderbook: OrderbookData,
    pub timestamp: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TradeEvent {
    #[serde(rename = "marketSlug")]
    pub market_slug: String,
    pub side: String,
    pub price: f64,
    pub size: f64,
    pub timestamp: f64,
    #[serde(rename = "tradeId")]
    pub trade_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrderUpdate {
    #[serde(rename = "orderId")]
    pub order_id: String,
    #[serde(rename = "marketSlug")]
    pub market_slug: String,
    pub side: String,
    #[serde(default)]
    pub price: Option<f64>,
    pub size: f64,
    pub filled: f64,
    pub status: String,
    pub timestamp: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FillEvent {
    #[serde(rename = "orderId")]
    pub order_id: String,
    #[serde(rename = "marketSlug")]
    pub market_slug: String,
    pub side: String,
    pub price: f64,
    pub size: f64,
    pub timestamp: f64,
    #[serde(rename = "fillId")]
    pub fill_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MarketUpdateEvent {
    #[serde(rename = "marketSlug")]
    pub market_slug: String,
    #[serde(rename = "lastPrice", default)]
    pub last_price: Option<f64>,
    #[serde(rename = "volume24h", default)]
    pub volume_24h: Option<f64>,
    #[serde(rename = "priceChange24h", default)]
    pub price_change_24h: Option<f64>,
    pub timestamp: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AmmPriceEntry {
    #[serde(rename = "marketId")]
    pub market_id: i32,
    #[serde(rename = "marketAddress")]
    pub market_address: String,
    #[serde(rename = "yesPrice")]
    pub yes_price: f64,
    #[serde(rename = "noPrice")]
    pub no_price: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NewPriceData {
    #[serde(rename = "marketAddress")]
    pub market_address: String,
    #[serde(rename = "updatedPrices")]
    pub updated_prices: Vec<AmmPriceEntry>,
    #[serde(rename = "blockNumber")]
    pub block_number: i64,
    pub timestamp: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransactionEvent {
    #[serde(rename = "userId", default)]
    pub user_id: Option<i32>,
    #[serde(rename = "txHash", default)]
    pub tx_hash: Option<String>,
    pub status: String,
    pub source: String,
    pub timestamp: String,
    #[serde(rename = "marketAddress", default)]
    pub market_address: Option<String>,
    #[serde(rename = "marketSlug", default)]
    pub market_slug: Option<String>,
    #[serde(rename = "tokenId", default)]
    pub token_id: Option<String>,
    #[serde(rename = "conditionId", default)]
    pub condition_id: Option<String>,
    #[serde(rename = "amountContracts", default)]
    pub amount_contracts: Option<String>,
    #[serde(rename = "amountCollateral", default)]
    pub amount_collateral: Option<String>,
    #[serde(default)]
    pub price: Option<String>,
    #[serde(default)]
    pub side: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MarketCreatedEvent {
    pub slug: String,
    pub title: String,
    #[serde(rename = "type")]
    pub market_type: String,
    #[serde(rename = "groupSlug", default)]
    pub group_slug: Option<String>,
    #[serde(rename = "categoryIds", default)]
    pub category_ids: Vec<i32>,
    #[serde(rename = "createdAt")]
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MarketResolvedEvent {
    pub slug: String,
    #[serde(rename = "type")]
    pub market_type: String,
    #[serde(rename = "winningOutcome")]
    pub winning_outcome: String,
    #[serde(rename = "winningIndex")]
    pub winning_index: i32,
    #[serde(rename = "resolutionDate")]
    pub resolution_date: String,
}

#[derive(Clone)]
pub struct WebSocketClient {
    inner: Arc<WebSocketInner>,
}

struct WebSocketInner {
    config: RwLock<WebSocketConfig>,
    state: RwLock<WebSocketState>,
    logger: SharedLogger,
    subscriptions: RwLock<HashMap<String, SubscriptionOptions>>,
    handlers: RwLock<HashMap<String, Vec<HandlerEntry>>>,
    next_hid: AtomicU64,
    reconnect_attempts: AtomicUsize,
    reconnecting: AtomicBool,
    manual_disconnect: AtomicBool,
    socket: Mutex<Option<Arc<SocketIoClient>>>,
}

#[derive(Clone)]
struct HandlerEntry {
    id: u64,
    once: bool,
    callback: EventHandler,
}

struct SocketIoClient {
    namespace: String,
    writer: tokio::sync::Mutex<WsWrite>,
    ack_id: AtomicU64,
    ack_chans: tokio::sync::Mutex<HashMap<u64, oneshot::Sender<Value>>>,
    logger: SharedLogger,
}

impl WebSocketClient {
    pub fn new(config: Option<WebSocketConfig>) -> Self {
        let config = config.unwrap_or_default();
        let logger = config.logger.clone().unwrap_or_else(noop_logger);
        Self {
            inner: Arc::new(WebSocketInner {
                config: RwLock::new(config),
                state: RwLock::new(WebSocketState::Disconnected),
                logger,
                subscriptions: RwLock::new(HashMap::new()),
                handlers: RwLock::new(HashMap::new()),
                next_hid: AtomicU64::new(0),
                reconnect_attempts: AtomicUsize::new(0),
                reconnecting: AtomicBool::new(false),
                manual_disconnect: AtomicBool::new(false),
                socket: Mutex::new(None),
            }),
        }
    }

    pub fn state(&self) -> WebSocketState {
        self.inner
            .state
            .read()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
    }

    pub fn is_connected(&self) -> bool {
        self.state() == WebSocketState::Connected
            && self
                .inner
                .socket
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .is_some()
    }

    pub fn set_api_key(&self, api_key: impl Into<String>) {
        self.inner
            .config
            .write()
            .unwrap_or_else(|err| err.into_inner())
            .api_key = Some(api_key.into());

        if self.is_connected() {
            let this = self.clone();
            tokio::spawn(async move {
                if let Err(err) = this.disconnect().await {
                    this.inner
                        .logger
                        .error(&format!("WebSocket reconnect disconnect failed: {err}"));
                }
                if let Err(err) = this.connect().await {
                    this.inner.logger.error(&format!(
                        "WebSocket reconnect failed after API key update: {err}"
                    ));
                }
            });
        }
    }

    pub fn set_hmac_credentials(&self, hmac_credentials: HmacCredentials) {
        self.inner
            .config
            .write()
            .unwrap_or_else(|err| err.into_inner())
            .hmac_credentials = Some(hmac_credentials);

        if self.is_connected() {
            let this = self.clone();
            tokio::spawn(async move {
                if let Err(err) = this.disconnect().await {
                    this.inner
                        .logger
                        .error(&format!("WebSocket reconnect disconnect failed: {err}"));
                }
                if let Err(err) = this.connect().await {
                    this.inner.logger.error(&format!(
                        "WebSocket reconnect failed after HMAC credential update: {err}"
                    ));
                }
            });
        }
    }

    pub async fn connect(&self) -> Result<()> {
        {
            let mut state = self
                .inner
                .state
                .write()
                .unwrap_or_else(|err| err.into_inner());
            if matches!(
                *state,
                WebSocketState::Connecting | WebSocketState::Connected
            ) {
                return Ok(());
            }
            *state = WebSocketState::Connecting;
        }

        let config = self
            .inner
            .config
            .read()
            .unwrap_or_else(|err| err.into_inner())
            .clone();
        self.inner.logger.info("Connecting to WebSocket");

        let socket = match tokio::time::timeout(
            Duration::from_millis(config.timeout_ms),
            SocketIoClient::connect(self.inner.clone(), config.clone()),
        )
        .await
        .map_err(|_| {
            LimitlessError::WebSocket(format!("connection timeout after {}ms", config.timeout_ms))
        }) {
            Ok(result) => match result {
                Ok(socket) => socket,
                Err(err) => {
                    let mut state = self.inner.state.write().unwrap_or_else(|e| e.into_inner());
                    *state = WebSocketState::Error;
                    return Err(err);
                }
            },
            Err(err) => {
                let mut state = self.inner.state.write().unwrap_or_else(|e| e.into_inner());
                *state = WebSocketState::Error;
                return Err(err);
            }
        };

        {
            let mut guard = self
                .inner
                .socket
                .lock()
                .unwrap_or_else(|err| err.into_inner());
            *guard = Some(socket);
        }
        self.inner.reconnecting.store(false, Ordering::SeqCst);
        self.inner.manual_disconnect.store(false, Ordering::SeqCst);
        {
            let mut state = self
                .inner
                .state
                .write()
                .unwrap_or_else(|err| err.into_inner());
            *state = WebSocketState::Connected;
        }
        self.inner.reconnect_attempts.store(0, Ordering::SeqCst);
        self.resubscribe_all().await;
        self.inner.dispatch_local("connect", Value::Null);
        Ok(())
    }

    pub async fn disconnect(&self) -> Result<()> {
        self.inner.manual_disconnect.store(true, Ordering::SeqCst);
        let socket = self
            .inner
            .socket
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .take();
        {
            let mut state = self
                .inner
                .state
                .write()
                .unwrap_or_else(|err| err.into_inner());
            *state = WebSocketState::Disconnected;
        }

        if let Some(socket) = socket {
            socket.close().await?;
        }

        Ok(())
    }

    pub async fn subscribe(
        &self,
        channel: SubscriptionChannel,
        options: SubscriptionOptions,
    ) -> Result<()> {
        if !self.is_connected() {
            return Err(LimitlessError::WebSocket(
                "WebSocket not connected. Call connect() first".to_string(),
            ));
        }

        let config = self
            .inner
            .config
            .read()
            .unwrap_or_else(|err| err.into_inner())
            .clone();
        if requires_websocket_auth(channel)
            && config.api_key.as_deref().unwrap_or("").is_empty()
            && config.hmac_credentials.is_none()
        {
            return Err(LimitlessError::AuthenticationRequired {
                operation: format!("'{}' subscription", channel.as_str()),
            });
        }

        let normalized = normalize_subscription_options(options);
        let key = subscription_key(channel, &normalized);
        self.inner
            .subscriptions
            .write()
            .unwrap_or_else(|err| err.into_inner())
            .insert(key, normalized.clone());

        let socket = self.require_socket()?;
        socket.emit(channel.as_str(), Some(&normalized)).await?;
        Ok(())
    }

    pub async fn unsubscribe(
        &self,
        channel: SubscriptionChannel,
        options: SubscriptionOptions,
    ) -> Result<()> {
        if !self.is_connected() {
            return Err(LimitlessError::WebSocket(
                "WebSocket not connected".to_string(),
            ));
        }

        let normalized = normalize_subscription_options(options);
        let key = subscription_key(channel, &normalized);
        self.inner
            .subscriptions
            .write()
            .unwrap_or_else(|err| err.into_inner())
            .remove(&key);

        let mut payload = serde_json::Map::new();
        payload.insert(
            "channel".to_string(),
            Value::String(channel.as_str().to_string()),
        );
        if let Some(value) = normalized.market_slug {
            payload.insert("marketSlug".to_string(), Value::String(value));
        }
        if !normalized.market_slugs.is_empty() {
            payload.insert(
                "marketSlugs".to_string(),
                serde_json::to_value(normalized.market_slugs).unwrap_or(Value::Null),
            );
        }
        if let Some(value) = normalized.market_address {
            payload.insert("marketAddress".to_string(), Value::String(value));
        }
        if !normalized.market_addresses.is_empty() {
            payload.insert(
                "marketAddresses".to_string(),
                serde_json::to_value(normalized.market_addresses).unwrap_or(Value::Null),
            );
        }
        if !normalized.filters.is_empty() {
            payload.insert(
                "filters".to_string(),
                serde_json::to_value(normalized.filters).unwrap_or(Value::Null),
            );
        }

        let socket = self.require_socket()?;
        let response = socket
            .emit_with_ack(
                "unsubscribe",
                Some(&Value::Object(payload)),
                Duration::from_secs(5),
            )
            .await?;
        if let Some(error) = response.get("error") {
            return Err(LimitlessError::WebSocket(format!(
                "unsubscribe failed: {error}"
            )));
        }
        Ok(())
    }

    pub fn on<F>(&self, event: &str, handler: F) -> u64
    where
        F: Fn(Value) + Send + Sync + 'static,
    {
        let id = self.inner.next_hid.fetch_add(1, Ordering::SeqCst) + 1;
        self.inner
            .handlers
            .write()
            .unwrap_or_else(|err| err.into_inner())
            .entry(event.to_string())
            .or_default()
            .push(HandlerEntry {
                id,
                once: false,
                callback: Arc::new(handler),
            });
        id
    }

    pub fn once<F>(&self, event: &str, handler: F) -> u64
    where
        F: Fn(Value) + Send + Sync + 'static,
    {
        let id = self.inner.next_hid.fetch_add(1, Ordering::SeqCst) + 1;
        self.inner
            .handlers
            .write()
            .unwrap_or_else(|err| err.into_inner())
            .entry(event.to_string())
            .or_default()
            .push(HandlerEntry {
                id,
                once: true,
                callback: Arc::new(handler),
            });
        id
    }

    pub fn off(&self, event: &str, handler_ids: &[u64]) {
        let mut handlers = self
            .inner
            .handlers
            .write()
            .unwrap_or_else(|err| err.into_inner());
        if handler_ids.is_empty() {
            handlers.remove(event);
            return;
        }

        if let Some(entries) = handlers.get_mut(event) {
            entries.retain(|entry| !handler_ids.contains(&entry.id));
        }
        let should_remove = handlers
            .get(event)
            .map(|entries| entries.is_empty())
            .unwrap_or(false);
        if should_remove {
            handlers.remove(event);
        }
    }

    pub fn on_orderbook_update<F>(&self, handler: F) -> u64
    where
        F: Fn(OrderbookUpdate) + Send + Sync + 'static,
    {
        self.on_typed("orderbookUpdate", "orderbook update", handler)
    }

    pub fn on_new_price_data<F>(&self, handler: F) -> u64
    where
        F: Fn(NewPriceData) + Send + Sync + 'static,
    {
        self.on_typed("newPriceData", "price data", handler)
    }

    pub fn on_trade<F>(&self, handler: F) -> u64
    where
        F: Fn(TradeEvent) + Send + Sync + 'static,
    {
        self.on_typed("trade", "trade event", handler)
    }

    pub fn on_order<F>(&self, handler: F) -> u64
    where
        F: Fn(OrderUpdate) + Send + Sync + 'static,
    {
        self.on_typed("order", "order event", handler)
    }

    pub fn on_fill<F>(&self, handler: F) -> u64
    where
        F: Fn(FillEvent) + Send + Sync + 'static,
    {
        self.on_typed("fill", "fill event", handler)
    }

    pub fn on_transaction<F>(&self, handler: F) -> u64
    where
        F: Fn(TransactionEvent) + Send + Sync + 'static,
    {
        self.on_typed("tx", "transaction event", handler)
    }

    pub fn on_market<F>(&self, handler: F) -> u64
    where
        F: Fn(MarketUpdateEvent) + Send + Sync + 'static,
    {
        self.on_typed("market", "market event", handler)
    }

    pub fn on_market_created<F>(&self, handler: F) -> u64
    where
        F: Fn(MarketCreatedEvent) + Send + Sync + 'static,
    {
        self.on_typed("marketCreated", "marketCreated event", handler)
    }

    pub fn on_market_resolved<F>(&self, handler: F) -> u64
    where
        F: Fn(MarketResolvedEvent) + Send + Sync + 'static,
    {
        self.on_typed("marketResolved", "marketResolved event", handler)
    }

    fn on_typed<T, F>(&self, event: &str, label: &'static str, handler: F) -> u64
    where
        T: for<'de> Deserialize<'de> + Send + Sync + 'static,
        F: Fn(T) + Send + Sync + 'static,
    {
        let logger = self.inner.logger.clone();
        self.on(event, move |data| match serde_json::from_value::<T>(data) {
            Ok(parsed) => handler(parsed),
            Err(err) => logger.error(&format!("Failed to parse {label}: {err}")),
        })
    }

    fn require_socket(&self) -> Result<Arc<SocketIoClient>> {
        self.inner
            .socket
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
            .ok_or_else(|| LimitlessError::WebSocket("WebSocket not connected".to_string()))
    }

    async fn resubscribe_all(&self) {
        let subscriptions = self
            .inner
            .subscriptions
            .read()
            .unwrap_or_else(|err| err.into_inner())
            .clone();

        for (key, options) in subscriptions {
            if let Some(channel) = channel_from_key(&key) {
                if let Ok(socket) = self.require_socket() {
                    let _ = socket.emit(channel.as_str(), Some(&options)).await;
                }
            } else {
                self.inner.logger.warn(&format!(
                    "Skipping unknown subscription key during resubscribe: {key}"
                ));
            }
        }
    }
}

impl WebSocketInner {
    fn dispatch_local(&self, event: &str, data: Value) {
        let entries = self
            .handlers
            .read()
            .unwrap_or_else(|err| err.into_inner())
            .get(event)
            .cloned()
            .unwrap_or_default();
        if entries.is_empty() {
            return;
        }

        let mut once_ids = Vec::new();
        for entry in entries {
            (entry.callback)(data.clone());
            if entry.once {
                once_ids.push(entry.id);
            }
        }

        if !once_ids.is_empty() {
            let mut handlers = self.handlers.write().unwrap_or_else(|err| err.into_inner());
            if let Some(entries) = handlers.get_mut(event) {
                entries.retain(|entry| !once_ids.contains(&entry.id));
            }
            let should_remove = handlers
                .get(event)
                .map(|entries| entries.is_empty())
                .unwrap_or(false);
            if should_remove {
                handlers.remove(event);
            }
        }
    }

    fn on_socket_disconnected(self: &Arc<Self>, reason: String) {
        {
            let mut socket = self.socket.lock().unwrap_or_else(|err| err.into_inner());
            *socket = None;
        }
        {
            let mut state = self.state.write().unwrap_or_else(|err| err.into_inner());
            *state = WebSocketState::Disconnected;
        }

        self.logger
            .warn(&format!("WebSocket disconnected: {reason}"));
        self.dispatch_local("disconnect", Value::String(reason));

        if self.manual_disconnect.swap(false, Ordering::SeqCst) {
            self.reconnecting.store(false, Ordering::SeqCst);
            return;
        }

        let config = self
            .config
            .read()
            .unwrap_or_else(|err| err.into_inner())
            .clone();
        if config.auto_reconnect && !self.reconnecting.swap(true, Ordering::SeqCst) {
            let inner = self.clone();
            tokio::spawn(async move {
                inner.reconnect_loop().await;
            });
        }
    }

    async fn reconnect_loop(self: Arc<Self>) {
        {
            let mut state = self.state.write().unwrap_or_else(|err| err.into_inner());
            *state = WebSocketState::Reconnecting;
        }

        let config = self
            .config
            .read()
            .unwrap_or_else(|err| err.into_inner())
            .clone();
        let mut delay_ms = config.reconnect_delay_ms.max(1);

        loop {
            let attempt = self.reconnect_attempts.fetch_add(1, Ordering::SeqCst) + 1;
            if config.max_reconnect_attempts > 0 && attempt > config.max_reconnect_attempts as usize
            {
                let mut state = self.state.write().unwrap_or_else(|err| err.into_inner());
                *state = WebSocketState::Error;
                self.reconnecting.store(false, Ordering::SeqCst);
                self.logger.error("Max reconnection attempts reached");
                return;
            }

            self.dispatch_local(
                "reconnecting",
                Value::Number(serde_json::Number::from(attempt as u64)),
            );

            match SocketIoClient::connect(self.clone(), config.clone()).await {
                Ok(socket) => {
                    {
                        let mut guard = self.socket.lock().unwrap_or_else(|err| err.into_inner());
                        *guard = Some(socket.clone());
                    }
                    {
                        let mut state = self.state.write().unwrap_or_else(|err| err.into_inner());
                        *state = WebSocketState::Connected;
                    }
                    self.reconnect_attempts.store(0, Ordering::SeqCst);
                    self.reconnecting.store(false, Ordering::SeqCst);
                    let subscriptions = self
                        .subscriptions
                        .read()
                        .unwrap_or_else(|err| err.into_inner())
                        .clone();
                    for (key, options) in subscriptions {
                        if let Some(channel) = channel_from_key(&key) {
                            let _ = socket.emit(channel.as_str(), Some(&options)).await;
                        } else {
                            self.logger.warn(&format!(
                                "Skipping unknown subscription key during reconnect: {key}"
                            ));
                        }
                    }
                    self.dispatch_local("connect", Value::Null);
                    return;
                }
                Err(err) => {
                    self.logger.error(&format!("Reconnection failed: {err}"));
                    sleep(Duration::from_millis(delay_ms)).await;
                    delay_ms = (delay_ms.saturating_mul(2)).min(60_000);
                }
            }
        }
    }
}

impl SocketIoClient {
    async fn connect(inner: Arc<WebSocketInner>, config: WebSocketConfig) -> Result<Arc<Self>> {
        let request = build_socket_io_request(&config)?;
        let (stream, _) = connect_async(request)
            .await
            .map_err(|err| LimitlessError::WebSocket(format!("websocket dial failed: {err}")))?;
        let (write, mut read) = stream.split();

        let open_packet = next_text_message(&mut read).await?;
        if !open_packet.starts_with('0') {
            return Err(LimitlessError::WebSocket(format!(
                "expected Engine.IO open packet, got: {open_packet}"
            )));
        }
        let _: Value = serde_json::from_str(&open_packet[1..]).map_err(|err| {
            LimitlessError::WebSocket(format!("failed to parse Engine.IO config: {err}"))
        })?;

        let socket = Arc::new(Self {
            namespace: SOCKET_NAMESPACE.to_string(),
            writer: tokio::sync::Mutex::new(write),
            ack_id: AtomicU64::new(0),
            ack_chans: tokio::sync::Mutex::new(HashMap::new()),
            logger: inner.logger.clone(),
        });

        socket
            .write_message(format!("40{},", SOCKET_NAMESPACE))
            .await?;

        let connect_ack = next_text_message(&mut read).await?;
        let expected = format!("40{},", SOCKET_NAMESPACE);
        if !connect_ack.starts_with(&expected) {
            return Err(LimitlessError::WebSocket(format!(
                "expected Socket.IO connect ack for namespace {}, got: {connect_ack}",
                SOCKET_NAMESPACE
            )));
        }

        let socket_clone = socket.clone();
        tokio::spawn(async move {
            socket_clone.read_loop(read, inner).await;
        });

        Ok(socket)
    }

    async fn emit<T: Serialize>(&self, event: &str, data: Option<&T>) -> Result<()> {
        let payload = if let Some(data) = data {
            serde_json::to_string(&serde_json::json!([event, data]))
        } else {
            serde_json::to_string(&serde_json::json!([event]))
        }
        .map_err(|err| LimitlessError::WebSocket(format!("failed to marshal emit data: {err}")))?;

        self.write_message(format!("42{},{}", self.namespace, payload))
            .await
    }

    async fn emit_with_ack<T: Serialize>(
        &self,
        event: &str,
        data: Option<&T>,
        timeout: Duration,
    ) -> Result<Value> {
        let ack_id = self.ack_id.fetch_add(1, Ordering::SeqCst) + 1;
        let (sender, receiver) = oneshot::channel();
        self.ack_chans.lock().await.insert(ack_id, sender);

        let payload = if let Some(data) = data {
            serde_json::to_string(&serde_json::json!([event, data]))
        } else {
            serde_json::to_string(&serde_json::json!([event]))
        }
        .map_err(|err| LimitlessError::WebSocket(format!("failed to marshal emit data: {err}")))?;

        if let Err(err) = self
            .write_message(format!("42{ack_id}{},{}", self.namespace, payload))
            .await
        {
            self.ack_chans.lock().await.remove(&ack_id);
            return Err(err);
        }

        match tokio::time::timeout(timeout, receiver).await {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(_)) => Err(LimitlessError::WebSocket(
                "connection closed while waiting for ack".to_string(),
            )),
            Err(_) => {
                self.ack_chans.lock().await.remove(&ack_id);
                Err(LimitlessError::WebSocket(format!(
                    "ack timeout after {:?}",
                    timeout
                )))
            }
        }
    }

    async fn close(&self) -> Result<()> {
        let _ = self.write_message(format!("41{},", self.namespace)).await;
        let mut writer = self.writer.lock().await;
        writer
            .send(Message::Close(None))
            .await
            .map_err(|err| LimitlessError::WebSocket(format!("failed to close socket: {err}")))
    }

    async fn read_loop(self: Arc<Self>, mut read: WsRead, inner: Arc<WebSocketInner>) {
        while let Some(message) = read.next().await {
            match message {
                Ok(Message::Text(text)) => {
                    if let Err(err) = self.handle_message(&text, &inner).await {
                        inner.on_socket_disconnected(err.to_string());
                        return;
                    }
                }
                Ok(Message::Binary(bytes)) => {
                    if let Ok(text) = String::from_utf8(bytes.to_vec()) {
                        if let Err(err) = self.handle_message(&text, &inner).await {
                            inner.on_socket_disconnected(err.to_string());
                            return;
                        }
                    }
                }
                Ok(Message::Close(_)) => {
                    inner.on_socket_disconnected("server close".to_string());
                    return;
                }
                Ok(Message::Ping(payload)) => {
                    let _ = self.writer.lock().await.send(Message::Pong(payload)).await;
                }
                Ok(_) => {}
                Err(err) => {
                    inner.on_socket_disconnected(format!("websocket read error: {err}"));
                    return;
                }
            }
        }

        inner.on_socket_disconnected("connection closed".to_string());
    }

    async fn handle_message(&self, message: &str, inner: &Arc<WebSocketInner>) -> Result<()> {
        match message {
            "3" => Ok(()),
            "2" => self.write_message("3".to_string()).await,
            "1" => {
                inner.on_socket_disconnected("server close".to_string());
                Ok(())
            }
            _ if message.starts_with('4') => {
                self.handle_socketio_packet(&message[1..], inner).await
            }
            _ => Ok(()),
        }
    }

    async fn handle_socketio_packet(
        &self,
        packet: &str,
        inner: &Arc<WebSocketInner>,
    ) -> Result<()> {
        if packet.is_empty() {
            return Ok(());
        }

        let packet_type = packet.as_bytes()[0] as char;
        let rest = &packet[1..];
        match packet_type {
            '0' => Ok(()),
            '1' => {
                inner.on_socket_disconnected("namespace disconnect".to_string());
                Ok(())
            }
            '2' => {
                let data = strip_namespace_prefix(rest, &self.namespace);
                let (event, payload) = parse_socketio_event(data)?;
                inner.dispatch_local(&event, payload);
                Ok(())
            }
            '3' => self.handle_ack(rest).await,
            '4' => {
                inner.dispatch_local("error", Value::String(rest.to_string()));
                Ok(())
            }
            _ => Ok(()),
        }
    }

    async fn handle_ack(&self, payload: &str) -> Result<()> {
        let digit_len = payload.chars().take_while(|ch| ch.is_ascii_digit()).count();
        if digit_len == 0 {
            return Ok(());
        }

        let ack_id = payload[..digit_len]
            .parse::<u64>()
            .map_err(|err| LimitlessError::WebSocket(format!("invalid ack id: {err}")))?;
        let rest = &payload[digit_len..];
        let value = if rest.is_empty() {
            Value::Null
        } else {
            let parsed: Vec<Value> = serde_json::from_str(rest)
                .unwrap_or_else(|_| vec![Value::String(rest.to_string())]);
            parsed.into_iter().next().unwrap_or(Value::Null)
        };

        if let Some(sender) = self.ack_chans.lock().await.remove(&ack_id) {
            let _ = sender.send(value);
        }
        Ok(())
    }

    async fn write_message(&self, packet: String) -> Result<()> {
        self.logger.debug(&format!("WebSocket send: {packet}"));
        self.writer
            .lock()
            .await
            .send(Message::Text(packet))
            .await
            .map_err(|err| {
                LimitlessError::WebSocket(format!("failed to send websocket packet: {err}"))
            })
    }
}

fn build_socket_io_request(config: &WebSocketConfig) -> Result<Request<()>> {
    let url = format!("{}{}", config.url.trim_end_matches('/'), SOCKET_IO_PATH);
    let headers =
        build_websocket_headers(config.api_key.as_deref(), config.hmac_credentials.as_ref())?;

    let mut builder = Request::builder().method("GET").uri(url);
    for (name, value) in headers.iter() {
        builder = builder.header(name, value.clone());
    }

    builder.body(()).map_err(|err| {
        LimitlessError::WebSocket(format!("failed to build websocket request: {err}"))
    })
}

fn build_websocket_headers(
    api_key: Option<&str>,
    hmac_credentials: Option<&HmacCredentials>,
) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    let version = env!("CARGO_PKG_VERSION");
    headers.insert(
        "user-agent",
        HeaderValue::from_str(&format!("{SDK_ID}/{version}"))
            .map_err(|err| LimitlessError::WebSocket(err.to_string()))?,
    );
    headers.insert(
        "x-sdk-version",
        HeaderValue::from_str(&format!("{SDK_ID}/{version}"))
            .map_err(|err| LimitlessError::WebSocket(err.to_string()))?,
    );

    if let Some(creds) = hmac_credentials {
        let timestamp = chrono_timestamp();
        let signature =
            compute_hmac_signature(&creds.secret, &timestamp, "GET", SOCKET_IO_PATH, "")?;
        headers.insert(
            "lmts-api-key",
            HeaderValue::from_str(&creds.token_id)
                .map_err(|err| LimitlessError::WebSocket(err.to_string()))?,
        );
        headers.insert(
            "lmts-timestamp",
            HeaderValue::from_str(&timestamp)
                .map_err(|err| LimitlessError::WebSocket(err.to_string()))?,
        );
        headers.insert(
            "lmts-signature",
            HeaderValue::from_str(&signature)
                .map_err(|err| LimitlessError::WebSocket(err.to_string()))?,
        );
    } else if let Some(api_key) = api_key.filter(|value| !value.trim().is_empty()) {
        headers.insert(
            "X-API-Key",
            HeaderValue::from_str(api_key)
                .map_err(|err| LimitlessError::WebSocket(err.to_string()))?,
        );
    }

    Ok(headers)
}

fn normalize_subscription_options(mut options: SubscriptionOptions) -> SubscriptionOptions {
    options.market_slugs.sort();
    options.market_addresses.sort();
    options
}

fn subscription_key(channel: SubscriptionChannel, options: &SubscriptionOptions) -> String {
    let normalized = normalize_subscription_options(options.clone());
    let encoded = serde_json::to_string(&normalized).unwrap_or_else(|_| String::new());
    format!("{}|{}", channel.as_str(), encoded)
}

fn channel_from_key(key: &str) -> Option<SubscriptionChannel> {
    match key.split('|').next().unwrap_or_default() {
        "orderbook" => Some(SubscriptionChannel::Orderbook),
        "trades" => Some(SubscriptionChannel::Trades),
        "orders" => Some(SubscriptionChannel::Orders),
        "fills" => Some(SubscriptionChannel::Fills),
        "markets" => Some(SubscriptionChannel::Markets),
        "prices" => Some(SubscriptionChannel::Prices),
        "subscribe_market_prices" => Some(SubscriptionChannel::SubscribeMarketPrices),
        "subscribe_positions" => Some(SubscriptionChannel::SubscribePositions),
        "subscribe_transactions" => Some(SubscriptionChannel::SubscribeTransactions),
        _ => None,
    }
}

fn requires_websocket_auth(channel: SubscriptionChannel) -> bool {
    matches!(
        channel,
        SubscriptionChannel::Orders
            | SubscriptionChannel::Fills
            | SubscriptionChannel::SubscribePositions
            | SubscriptionChannel::SubscribeTransactions
    )
}

async fn next_text_message(read: &mut WsRead) -> Result<String> {
    match read.next().await {
        Some(Ok(Message::Text(text))) => Ok(text.to_string()),
        Some(Ok(Message::Binary(bytes))) => String::from_utf8(bytes.to_vec())
            .map_err(|err| LimitlessError::WebSocket(format!("invalid text frame: {err}"))),
        Some(Ok(other)) => Err(LimitlessError::WebSocket(format!(
            "expected text frame, got {other:?}"
        ))),
        Some(Err(err)) => Err(LimitlessError::WebSocket(format!(
            "failed to read websocket frame: {err}"
        ))),
        None => Err(LimitlessError::WebSocket(
            "websocket connection closed during handshake".to_string(),
        )),
    }
}

fn strip_namespace_prefix<'a>(payload: &'a str, namespace: &str) -> &'a str {
    let digit_len = payload.chars().take_while(|ch| ch.is_ascii_digit()).count();
    let without_ack = &payload[digit_len..];
    if let Some(stripped) = without_ack.strip_prefix(&format!("{namespace},")) {
        stripped
    } else {
        without_ack
    }
}

fn parse_socketio_event(payload: &str) -> Result<(String, Value)> {
    let values: Vec<Value> = serde_json::from_str(payload).map_err(|err| {
        LimitlessError::WebSocket(format!("failed to parse Socket.IO event: {err}"))
    })?;
    if values.is_empty() {
        return Err(LimitlessError::WebSocket(
            "Socket.IO event payload is empty".to_string(),
        ));
    }
    let event = values[0]
        .as_str()
        .ok_or_else(|| {
            LimitlessError::WebSocket("Socket.IO event name is not a string".to_string())
        })?
        .to_string();
    let data = values.get(1).cloned().unwrap_or(Value::Null);
    Ok((event, data))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn websocket_headers_include_sdk_tracking() {
        let headers = build_websocket_headers(None, None).expect("headers should build");
        assert!(headers.get("x-sdk-version").is_some());
        assert!(headers.get("user-agent").is_some());
        assert!(headers.get("X-API-Key").is_none());
    }

    #[test]
    fn websocket_headers_include_hmac_headers() {
        let headers = build_websocket_headers(
            None,
            Some(&HmacCredentials {
                token_id: "token-123".to_string(),
                secret: "c2VjcmV0".to_string(),
            }),
        )
        .expect("headers should build");
        assert_eq!(
            headers.get("lmts-api-key").and_then(|v| v.to_str().ok()),
            Some("token-123")
        );
        assert!(headers.get("lmts-signature").is_some());
    }

    #[test]
    fn subscription_key_is_order_independent() {
        let left = subscription_key(
            SubscriptionChannel::SubscribeMarketPrices,
            &SubscriptionOptions {
                market_slugs: vec!["eth".to_string(), "btc".to_string()],
                ..Default::default()
            },
        );
        let right = subscription_key(
            SubscriptionChannel::SubscribeMarketPrices,
            &SubscriptionOptions {
                market_slugs: vec!["btc".to_string(), "eth".to_string()],
                ..Default::default()
            },
        );
        assert_eq!(left, right);
    }

    #[test]
    fn orderbook_update_parses_string_encoded_scalars() {
        let parsed: OrderbookUpdate = serde_json::from_str(
            r#"{
                "marketSlug":"btc",
                "orderbook":{
                    "bids":[{"price":0.51,"size":100.0,"side":"buy"}],
                    "asks":[{"price":0.52,"size":120.0,"side":"sell"}],
                    "tokenId":"123",
                    "adjustedMidpoint":0.515,
                    "maxSpread":"0.05",
                    "minSize":"100000000"
                },
                "timestamp":"2026-03-17T00:00:00.000Z"
            }"#,
        )
        .expect("payload should parse");

        assert_eq!(parsed.orderbook.max_spread.float64(), 0.05);
        assert_eq!(parsed.orderbook.min_size.float64(), 100_000_000.0);
    }

    #[test]
    fn unknown_subscription_key_is_rejected() {
        assert!(channel_from_key("mystery|{}").is_none());
    }
}
