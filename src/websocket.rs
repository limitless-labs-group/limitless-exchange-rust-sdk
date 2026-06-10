use std::{
    collections::{BTreeMap, HashMap},
    fmt::Write as _,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex, RwLock,
    },
    time::Duration,
};

use futures_util::{sink::SinkExt, stream::SplitSink, stream::SplitStream, StreamExt};
use http::{HeaderMap, HeaderValue, Request};
use once_cell::sync::Lazy;
use rustls::{ClientConfig as RustlsClientConfig, RootCertStore};
use rustls_pki_types::ServerName;
use serde::{de::Error as DeError, Deserialize, Deserializer, Serialize};
use serde_json::Value;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::oneshot,
    time::sleep,
};
use tokio_rustls::TlsConnector;
use tokio_tungstenite::{
    tungstenite::{
        client::IntoClientRequest,
        handshake::{client::generate_key, derive_accept_key},
        protocol::Role,
        Message,
    },
    MaybeTlsStream, WebSocketStream,
};

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
const MAX_HANDSHAKE_RESPONSE_BYTES: usize = 64 * 1024;

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;
type WsWrite = SplitSink<WsStream, Message>;
type WsRead = SplitStream<WsStream>;
type EventHandler = Arc<dyn Fn(Value) + Send + Sync>;

static DEFAULT_TLS_CONFIG: Lazy<Arc<RustlsClientConfig>> = Lazy::new(|| {
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    Arc::new(
        RustlsClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .expect("ring provider should support rustls safe default protocol versions")
        .with_root_certificates(roots)
        .with_no_client_auth(),
    )
});

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
    SubscribeMarketPrices,
    SubscribePositions,
    SubscribeTransactions,
    SubscribeOrderEvents,
    SubscribeLiveSports,
    SubscribeLiveEsports,
    SubscribeMarketLifecycle,
    UnsubscribeMarketLifecycle,
}

impl SubscriptionChannel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SubscribeMarketPrices => "subscribe_market_prices",
            Self::SubscribePositions => "subscribe_positions",
            Self::SubscribeTransactions => "subscribe_transactions",
            Self::SubscribeOrderEvents => "subscribe_order_events",
            Self::SubscribeLiveSports => "subscribe_live_sports",
            Self::SubscribeLiveEsports => "subscribe_live_esports",
            Self::SubscribeMarketLifecycle => "subscribe_market_lifecycle",
            Self::UnsubscribeMarketLifecycle => "unsubscribe_market_lifecycle",
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

/// Single AMM price entry in `newPriceData.updatedPrices`.
///
/// CLOB `orderbookUpdate` events use `OrderbookData` instead.
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
pub struct OraclePriceData {
    #[serde(rename = "marketAddress", default)]
    pub market_address: Option<String>,
    #[serde(rename = "marketSlug")]
    pub market_slug: String,
    pub timestamp: i64,
    pub value: f64,
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

/// Typed `orderEvent` payload.
///
/// All `orderEvent` frames arrive on the same socket.io event and are
/// discriminated on the `type` field. `MATCHED` is the pre-settlement
/// per-fill estimate (`source: "SETTLEMENT"`); `EXECUTION` is the FAK/FOK
/// terminal outcome (`source: "OME"`). Lifecycle frames
/// (`PLACEMENT`/`UPDATE`/`CANCELLATION`/`MINED`/`FAILED`) fall into `Unknown`;
/// use the raw [`WebSocketClient::on_order_event`] handler for those.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum OrderEvent {
    #[serde(rename = "MATCHED")]
    Matched(MatchedOrderEvent),
    #[serde(rename = "EXECUTION")]
    Execution(ExecutionOrderEvent),
    #[serde(other)]
    Unknown,
}

/// Pre-settlement per-fill `MATCHED` frame (`source: "SETTLEMENT"`).
///
/// Monetary fields are JSON strings as emitted by settlement.
/// `configured_fee_rate_bps` / `effective_fee_bps` are JSON numbers; the maker
/// side reports `0`, the taker side a real estimate. `is_estimate` is `true`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchedOrderEvent {
    pub source: String,
    pub event_id: String,
    pub price: String,
    #[serde(default)]
    pub amount_contracts: Option<String>,
    #[serde(default)]
    pub amount_collateral: Option<String>,
    #[serde(default)]
    pub fee_amount_contracts: Option<String>,
    #[serde(default)]
    pub fee_amount_collateral: Option<String>,
    #[serde(default)]
    pub configured_fee_rate_bps: Option<f64>,
    #[serde(default)]
    pub effective_fee_bps: Option<f64>,
    #[serde(default)]
    pub side: Option<String>,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub token_id: Option<String>,
    #[serde(default)]
    pub market_slug: Option<String>,
    #[serde(default)]
    pub order_id: Option<String>,
    #[serde(default)]
    pub taker_order_id: Option<String>,
    #[serde(default)]
    pub trade_event_id: Option<String>,
    #[serde(default)]
    pub is_estimate: Option<bool>,
    pub timestamp: String,
}

/// FAK/FOK terminal `EXECUTION` frame (`source: "OME"`).
///
/// `price` and `remaining_size` arrive as JSON numbers from OME (the
/// [`FlexFloat`] newtype also tolerates string encodings). `event_id` is the
/// string form `"terminal:<orderId>"`. `status` is `FILLED`,
/// `PARTIALLY_FILLED`, or `KILLED`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionOrderEvent {
    pub source: String,
    pub event_id: String,
    pub status: String,
    pub price: FlexFloat,
    pub remaining_size: FlexFloat,
    pub token: String,
    pub side: String,
    pub market_id: String,
    pub order_id: String,
    pub timestamp: String,
    #[serde(default)]
    pub user_id: Option<i64>,
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

    pub fn on_oracle_price_data<F>(&self, handler: F) -> u64
    where
        F: Fn(OraclePriceData) + Send + Sync + 'static,
    {
        self.on_typed("oraclePriceData", "oracle price data", handler)
    }

    pub fn on_order_event<F>(&self, handler: F) -> u64
    where
        F: Fn(Value) + Send + Sync + 'static,
    {
        self.on("orderEvent", handler)
    }

    pub fn on_order_event_typed<F>(&self, handler: F) -> u64
    where
        F: Fn(OrderEvent) + Send + Sync + 'static,
    {
        self.on_typed("orderEvent", "order event", handler)
    }

    pub fn on_transaction<F>(&self, handler: F) -> u64
    where
        F: Fn(TransactionEvent) + Send + Sync + 'static,
    {
        self.on_typed("tx", "transaction event", handler)
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

    pub fn on_live_sports_update<F>(&self, handler: F) -> u64
    where
        F: Fn(Value) + Send + Sync + 'static,
    {
        self.on("live_sports_update", handler)
    }

    pub fn on_live_esports_update<F>(&self, handler: F) -> u64
    where
        F: Fn(Value) + Send + Sync + 'static,
    {
        self.on("live_esports_update", handler)
    }

    pub fn on_system<F>(&self, handler: F) -> u64
    where
        F: Fn(Value) + Send + Sync + 'static,
    {
        self.on("system", handler)
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
        let stream = open_socket_stream(request.uri()).await?;
        let (stream, buffered) = perform_websocket_upgrade(stream, &request).await?;
        let stream =
            WebSocketStream::from_partially_read(stream, buffered, Role::Client, None).await;
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

async fn open_socket_stream(uri: &http::Uri) -> Result<MaybeTlsStream<TcpStream>> {
    let host = uri
        .host()
        .ok_or_else(|| LimitlessError::WebSocket("websocket URL is missing a host".to_string()))?;
    let scheme = uri.scheme_str().ok_or_else(|| {
        LimitlessError::WebSocket("websocket URL is missing a scheme".to_string())
    })?;
    let port = uri.port_u16().unwrap_or(match scheme {
        "wss" => 443,
        "ws" => 80,
        _ => 0,
    });

    if port == 0 {
        return Err(LimitlessError::WebSocket(format!(
            "unsupported websocket URL scheme: {scheme}"
        )));
    }

    let socket = TcpStream::connect(format!("{host}:{port}"))
        .await
        .map_err(|err| LimitlessError::WebSocket(format!("websocket tcp dial failed: {err}")))?;
    socket
        .set_nodelay(true)
        .map_err(|err| LimitlessError::WebSocket(format!("failed to disable Nagle: {err}")))?;

    match scheme {
        "ws" => Ok(MaybeTlsStream::Plain(socket)),
        "wss" => {
            let server_name = ServerName::try_from(host.to_string()).map_err(|err| {
                LimitlessError::WebSocket(format!(
                    "invalid websocket TLS server name {host}: {err}"
                ))
            })?;
            let tls = TlsConnector::from(DEFAULT_TLS_CONFIG.clone())
                .connect(server_name, socket)
                .await
                .map_err(|err| {
                    LimitlessError::WebSocket(format!("websocket TLS handshake failed: {err}"))
                })?;
            Ok(MaybeTlsStream::Rustls(tls))
        }
        _ => Err(LimitlessError::WebSocket(format!(
            "unsupported websocket URL scheme: {scheme}"
        ))),
    }
}

async fn perform_websocket_upgrade(
    mut stream: MaybeTlsStream<TcpStream>,
    request: &Request<()>,
) -> Result<(MaybeTlsStream<TcpStream>, Vec<u8>)> {
    let request_bytes = serialize_websocket_request(request)?;
    let accept_key = request
        .headers()
        .get("Sec-WebSocket-Key")
        .ok_or_else(|| {
            LimitlessError::WebSocket("websocket request is missing Sec-WebSocket-Key".to_string())
        })?
        .to_str()
        .map_err(|err| LimitlessError::WebSocket(format!("invalid websocket key header: {err}")))?
        .to_string();

    stream.write_all(&request_bytes).await.map_err(|err| {
        LimitlessError::WebSocket(format!("failed to write websocket upgrade request: {err}"))
    })?;
    stream.flush().await.map_err(|err| {
        LimitlessError::WebSocket(format!("failed to flush websocket upgrade request: {err}"))
    })?;

    let mut response = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        let read = stream.read(&mut chunk).await.map_err(|err| {
            LimitlessError::WebSocket(format!("failed to read websocket upgrade response: {err}"))
        })?;
        if read == 0 {
            return Err(LimitlessError::WebSocket(
                "websocket closed before upgrade completed".to_string(),
            ));
        }

        response.extend_from_slice(&chunk[..read]);
        if response.len() > MAX_HANDSHAKE_RESPONSE_BYTES {
            return Err(LimitlessError::WebSocket(format!(
                "websocket upgrade response exceeded {MAX_HANDSHAKE_RESPONSE_BYTES} bytes"
            )));
        }

        if let Some(end) = response
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .map(|index| index + 4)
        {
            let buffered = response.split_off(end);
            validate_websocket_upgrade_response(&response, &accept_key)?;
            return Ok((stream, buffered));
        }
    }
}

fn serialize_websocket_request(request: &Request<()>) -> Result<Vec<u8>> {
    const REQUIRED_HEADERS: [&str; 5] = [
        "Host",
        "Connection",
        "Upgrade",
        "Sec-WebSocket-Version",
        "Sec-WebSocket-Key",
    ];

    let mut output = String::new();
    let path_and_query = request
        .uri()
        .path_and_query()
        .ok_or_else(|| {
            LimitlessError::WebSocket("websocket request URL is missing a path".to_string())
        })?
        .as_str();

    write!(&mut output, "GET {path_and_query} HTTP/1.1\r\n").map_err(|err| {
        LimitlessError::WebSocket(format!("failed to build websocket request line: {err}"))
    })?;

    for header in REQUIRED_HEADERS {
        let value = request.headers().get(header).ok_or_else(|| {
            LimitlessError::WebSocket(format!(
                "websocket request is missing required header {header}"
            ))
        })?;
        write!(
            &mut output,
            "{header}: {}\r\n",
            value.to_str().map_err(|err| {
                LimitlessError::WebSocket(format!(
                    "websocket header {header} is not valid UTF-8: {err}"
                ))
            })?
        )
        .map_err(|err| {
            LimitlessError::WebSocket(format!("failed to write websocket header {header}: {err}"))
        })?;
    }

    for (name, value) in request.headers() {
        if REQUIRED_HEADERS
            .iter()
            .any(|required| name.as_str().eq_ignore_ascii_case(required))
        {
            continue;
        }

        let display_name = match name.as_str() {
            "origin" => "Origin",
            "sec-websocket-protocol" => "Sec-WebSocket-Protocol",
            other => other,
        };
        write!(
            &mut output,
            "{display_name}: {}\r\n",
            value.to_str().map_err(|err| {
                LimitlessError::WebSocket(format!(
                    "websocket header {display_name} is not valid UTF-8: {err}"
                ))
            })?
        )
        .map_err(|err| {
            LimitlessError::WebSocket(format!(
                "failed to write websocket header {display_name}: {err}"
            ))
        })?;
    }

    output.push_str("\r\n");
    Ok(output.into_bytes())
}

fn validate_websocket_upgrade_response(response: &[u8], request_key: &str) -> Result<()> {
    let response_text = std::str::from_utf8(response).map_err(|err| {
        LimitlessError::WebSocket(format!(
            "websocket upgrade response is not valid UTF-8: {err}"
        ))
    })?;
    let mut lines = response_text.split("\r\n");
    let status_line = lines.next().unwrap_or_default();
    let status_code = status_line.split_whitespace().nth(1).ok_or_else(|| {
        LimitlessError::WebSocket(format!(
            "websocket upgrade response is missing an HTTP status: {status_line}"
        ))
    })?;

    if status_code != "101" {
        return Err(LimitlessError::WebSocket(format!(
            "websocket upgrade rejected: {status_line}"
        )));
    }

    let mut upgrade_ok = false;
    let mut connection_ok = false;
    let mut accept_ok = false;
    let expected_accept = derive_accept_key(request_key.as_bytes());

    for line in lines {
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            let value = value.trim();
            match name.trim().to_ascii_lowercase().as_str() {
                "upgrade" => {
                    upgrade_ok = value.eq_ignore_ascii_case("websocket");
                }
                "connection" => {
                    connection_ok = value
                        .split(',')
                        .any(|token| token.trim().eq_ignore_ascii_case("upgrade"));
                }
                "sec-websocket-accept" => {
                    accept_ok = value == expected_accept;
                }
                _ => {}
            }
        }
    }

    if !upgrade_ok {
        return Err(LimitlessError::WebSocket(
            "websocket upgrade response is missing Upgrade: websocket".to_string(),
        ));
    }
    if !connection_ok {
        return Err(LimitlessError::WebSocket(
            "websocket upgrade response is missing Connection: Upgrade".to_string(),
        ));
    }
    if !accept_ok {
        return Err(LimitlessError::WebSocket(
            "websocket upgrade response has an invalid Sec-WebSocket-Accept header".to_string(),
        ));
    }

    Ok(())
}

fn build_socket_io_request(config: &WebSocketConfig) -> Result<Request<()>> {
    let url = format!("{}{}", config.url.trim_end_matches('/'), SOCKET_IO_PATH);
    let headers =
        build_websocket_headers(config.api_key.as_deref(), config.hmac_credentials.as_ref())?;

    let mut request = url.into_client_request().map_err(|err| {
        LimitlessError::WebSocket(format!("failed to build websocket client request: {err}"))
    })?;
    request.headers_mut().insert(
        "Sec-WebSocket-Key",
        HeaderValue::from_str(&generate_key()).map_err(|err| {
            LimitlessError::WebSocket(format!("failed to generate websocket key: {err}"))
        })?,
    );
    for (name, value) in headers.iter() {
        request.headers_mut().insert(name, value.clone());
    }

    Ok(request)
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
        "subscribe_market_prices" => Some(SubscriptionChannel::SubscribeMarketPrices),
        "subscribe_positions" => Some(SubscriptionChannel::SubscribePositions),
        "subscribe_transactions" => Some(SubscriptionChannel::SubscribeTransactions),
        "subscribe_order_events" => Some(SubscriptionChannel::SubscribeOrderEvents),
        "subscribe_live_sports" => Some(SubscriptionChannel::SubscribeLiveSports),
        "subscribe_live_esports" => Some(SubscriptionChannel::SubscribeLiveEsports),
        "subscribe_market_lifecycle" => Some(SubscriptionChannel::SubscribeMarketLifecycle),
        "unsubscribe_market_lifecycle" => Some(SubscriptionChannel::UnsubscribeMarketLifecycle),
        _ => None,
    }
}

fn requires_websocket_auth(channel: SubscriptionChannel) -> bool {
    matches!(
        channel,
        SubscriptionChannel::SubscribePositions
            | SubscriptionChannel::SubscribeTransactions
            | SubscriptionChannel::SubscribeOrderEvents
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
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

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
    fn websocket_request_includes_protocol_handshake_headers() {
        let request =
            build_socket_io_request(&WebSocketConfig::default()).expect("request should build");
        let headers = request.headers();

        assert!(headers.get("sec-websocket-key").is_some());
        assert!(headers.get("sec-websocket-version").is_some());
        assert!(headers.get("connection").is_some());
        assert!(headers.get("upgrade").is_some());
    }

    #[tokio::test]
    async fn websocket_manual_upgrade_writes_protocol_headers_on_the_wire() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener should bind");
        let addr = listener
            .local_addr()
            .expect("listener should have local addr");

        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("server should accept");
            let mut buf = vec![0_u8; 4096];
            let mut request = Vec::new();

            loop {
                let read = stream
                    .read(&mut buf)
                    .await
                    .expect("server should read request");
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buf[..read]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }

            let request_text = String::from_utf8(request.clone()).expect("request should be utf8");
            let key_line = request_text
                .lines()
                .find(|line| line.starts_with("Sec-WebSocket-Key: "))
                .expect("request should contain Sec-WebSocket-Key");
            let key = key_line
                .split_once(": ")
                .map(|(_, value)| value.trim())
                .expect("key header should contain a value")
                .to_string();
            let accept = derive_accept_key(key.as_bytes());

            stream
                .write_all(
                    format!(
                        "HTTP/1.1 101 Switching Protocols\r\n\
Upgrade: websocket\r\n\
Connection: Upgrade\r\n\
Sec-WebSocket-Accept: {accept}\r\n\r\n"
                    )
                    .as_bytes(),
                )
                .await
                .expect("server should write response");
            request
        });

        let request = build_socket_io_request(&WebSocketConfig {
            url: format!("ws://{}", addr),
            ..WebSocketConfig::default()
        })
        .expect("request should build");

        let stream = open_socket_stream(request.uri())
            .await
            .expect("socket should connect");
        let (_stream, buffered) = perform_websocket_upgrade(stream, &request)
            .await
            .expect("manual websocket upgrade should succeed");
        assert!(buffered.is_empty());

        let request_bytes = server.await.expect("server task should complete");
        let request_text = String::from_utf8(request_bytes).expect("request should be utf8");

        assert!(request_text.contains("Sec-WebSocket-Key: "));
        assert!(request_text.contains("Sec-WebSocket-Version: 13"));
        assert!(request_text.contains("Connection: Upgrade"));
        assert!(request_text.contains("Upgrade: websocket"));
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

    #[test]
    fn websocket_channel_inventory_includes_server_subscription_events() {
        let channels = [
            (
                SubscriptionChannel::SubscribeOrderEvents,
                "subscribe_order_events",
                true,
            ),
            (
                SubscriptionChannel::SubscribeLiveSports,
                "subscribe_live_sports",
                false,
            ),
            (
                SubscriptionChannel::SubscribeLiveEsports,
                "subscribe_live_esports",
                false,
            ),
            (
                SubscriptionChannel::SubscribeMarketLifecycle,
                "subscribe_market_lifecycle",
                false,
            ),
            (
                SubscriptionChannel::UnsubscribeMarketLifecycle,
                "unsubscribe_market_lifecycle",
                false,
            ),
        ];

        for (channel, wire_name, requires_auth) in channels {
            assert_eq!(channel.as_str(), wire_name);
            assert_eq!(
                channel_from_key(&format!("{wire_name}|{{}}")),
                Some(channel)
            );
            assert_eq!(requires_websocket_auth(channel), requires_auth);
        }
    }
}
