use std::{
    env,
    sync::{
        atomic::{AtomicI64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use hex::FromHex;
use k256::ecdsa::{RecoveryId, Signature, SigningKey};
use num_bigint::{BigInt, Sign};
use num_traits::{Signed, ToPrimitive, Zero};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{de::Error as DeError, Deserialize, Deserializer, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};
use sha3::{Digest, Keccak256};
use zeroize::Zeroizing;

use crate::{
    constants::{DEFAULT_CHAIN_ID, ZERO_ADDRESS},
    errors::{LimitlessError, Result},
    http_client::HttpClient,
    logger::{noop_logger, SharedLogger},
    markets::{MarketFetcher, Venue},
    portfolio::{PortfolioFetcher, UserProfile},
};

static NUMERIC_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\d+$").expect("valid numeric regex"));
static SIGNATURE_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^0x[0-9a-fA-F]{130}$").expect("valid signature regex"));

static DOMAIN_TYPEHASH: Lazy<[u8; 32]> = Lazy::new(|| {
    keccak256(b"EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)")
});
static ORDER_TYPEHASH: Lazy<[u8; 32]> = Lazy::new(|| {
    keccak256(
        b"Order(uint256 salt,address maker,address signer,address taker,uint256 tokenId,uint256 makerAmount,uint256 takerAmount,uint256 expiration,uint256 nonce,uint256 feeRateBps,uint8 side,uint8 signatureType)",
    )
});
static DOMAIN_NAME_HASH: Lazy<[u8; 32]> = Lazy::new(|| keccak256(b"Limitless CTF Exchange"));
static DOMAIN_VERSION_HASH: Lazy<[u8; 32]> = Lazy::new(|| keccak256(b"1"));
static LAST_ORDER_SALT: AtomicI64 = AtomicI64::new(0);

const DEFAULT_PRICE_TICK: f64 = 0.001;
const DEFAULT_FEE_RATE_BPS: i32 = 300;
const MAX_RECV_WINDOW_MS: i64 = 10_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize_repr, Deserialize_repr)]
#[repr(u8)]
pub enum Side {
    Buy = 0,
    Sell = 1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderType {
    #[serde(rename = "FOK")]
    Fok,
    #[serde(rename = "FAK")]
    Fak,
    #[serde(rename = "GTC")]
    Gtc,
}

/// Self-trade prevention policy applied when a taker order would match its own
/// resting maker orders.
///
/// Sent as the top-level `stpPolicy` field on the create-order request body. It
/// is not part of the signed EIP-712 order. Omit it to let the matching engine
/// apply its default (`cancel_maker`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StpPolicy {
    #[serde(rename = "cancel_both")]
    CancelBoth,
    #[serde(rename = "cancel_maker")]
    CancelMaker,
    #[serde(rename = "cancel_taker")]
    CancelTaker,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize_repr, Deserialize_repr)]
#[repr(u8)]
pub enum SignatureType {
    Eoa = 0,
    PolyProxy = 1,
    PolyGnosisSafe = 2,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FokOrderArgs {
    #[serde(rename = "tokenId")]
    pub token_id: String,
    pub side: Side,
    #[serde(rename = "makerAmount")]
    pub maker_amount: f64,
    #[serde(default)]
    pub expiration: Option<String>,
    #[serde(default)]
    pub nonce: Option<i32>,
    #[serde(default)]
    pub taker: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GtcOrderArgs {
    #[serde(rename = "tokenId")]
    pub token_id: String,
    pub side: Side,
    pub price: f64,
    pub size: f64,
    #[serde(default)]
    pub expiration: Option<String>,
    #[serde(default)]
    pub nonce: Option<i32>,
    #[serde(default)]
    pub taker: Option<String>,
    #[serde(rename = "postOnly", default)]
    pub post_only: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FakOrderArgs {
    #[serde(rename = "tokenId")]
    pub token_id: String,
    pub side: Side,
    pub price: f64,
    pub size: f64,
    #[serde(default)]
    pub expiration: Option<String>,
    #[serde(default)]
    pub nonce: Option<i32>,
    #[serde(default)]
    pub taker: Option<String>,
}

#[derive(Clone, Debug)]
pub enum OrderArgs {
    Fok(FokOrderArgs),
    Gtc(GtcOrderArgs),
    Fak(FakOrderArgs),
}

impl From<FokOrderArgs> for OrderArgs {
    fn from(value: FokOrderArgs) -> Self {
        Self::Fok(value)
    }
}

impl From<GtcOrderArgs> for OrderArgs {
    fn from(value: GtcOrderArgs) -> Self {
        Self::Gtc(value)
    }
}

impl From<FakOrderArgs> for OrderArgs {
    fn from(value: FakOrderArgs) -> Self {
        Self::Fak(value)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UnsignedOrder {
    pub salt: i64,
    pub maker: String,
    pub signer: String,
    pub taker: String,
    #[serde(rename = "tokenId")]
    pub token_id: String,
    #[serde(rename = "makerAmount")]
    pub maker_amount: i64,
    #[serde(rename = "takerAmount")]
    pub taker_amount: i64,
    pub expiration: String,
    pub nonce: i32,
    #[serde(rename = "feeRateBps")]
    pub fee_rate_bps: i32,
    pub side: Side,
    #[serde(rename = "signatureType")]
    pub signature_type: SignatureType,
    #[serde(default)]
    pub price: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SignedOrder {
    pub salt: i64,
    pub maker: String,
    pub signer: String,
    pub taker: String,
    #[serde(rename = "tokenId")]
    pub token_id: String,
    #[serde(rename = "makerAmount")]
    pub maker_amount: i64,
    #[serde(rename = "takerAmount")]
    pub taker_amount: i64,
    pub expiration: String,
    pub nonce: i32,
    #[serde(rename = "feeRateBps")]
    pub fee_rate_bps: i32,
    pub side: Side,
    #[serde(rename = "signatureType")]
    pub signature_type: SignatureType,
    #[serde(default)]
    pub price: Option<f64>,
    pub signature: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReceiveWindowOptions {
    /// Client-stamped order creation time in Unix milliseconds.
    ///
    /// This is sent as a top-level `POST /orders` field and is not signed.
    pub timestamp: Option<i64>,
    /// Maximum accepted request staleness in milliseconds.
    ///
    /// This is sent as top-level `recvWindow` and is not signed.
    pub recv_window: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NewOrderPayload {
    pub order: SignedOrder,
    #[serde(rename = "orderType")]
    pub order_type: OrderType,
    #[serde(rename = "marketSlug")]
    pub market_slug: String,
    #[serde(rename = "ownerId")]
    pub owner_id: i32,
    #[serde(rename = "postOnly", skip_serializing_if = "Option::is_none")]
    pub post_only: Option<bool>,
    #[serde(rename = "stpPolicy", skip_serializing_if = "Option::is_none")]
    pub stp_policy: Option<StpPolicy>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<i64>,
    #[serde(rename = "recvWindow", skip_serializing_if = "Option::is_none")]
    pub recv_window: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreatedOrder {
    pub id: String,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(
        rename = "makerAmount",
        deserialize_with = "deserialize_i64_from_number_or_string"
    )]
    pub maker_amount: i64,
    #[serde(
        rename = "takerAmount",
        deserialize_with = "deserialize_i64_from_number_or_string"
    )]
    pub taker_amount: i64,
    #[serde(default)]
    pub expiration: Option<String>,
    #[serde(rename = "signatureType")]
    pub signature_type: i32,
    #[serde(deserialize_with = "deserialize_i64_from_number_or_string")]
    pub salt: i64,
    pub maker: String,
    pub signer: String,
    pub taker: String,
    #[serde(rename = "tokenId")]
    pub token_id: String,
    pub side: Side,
    #[serde(rename = "feeRateBps")]
    pub fee_rate_bps: i32,
    pub nonce: i32,
    pub signature: String,
    #[serde(rename = "orderType")]
    pub order_type: String,
    #[serde(
        default,
        deserialize_with = "deserialize_option_f64_from_number_or_string"
    )]
    pub price: Option<f64>,
    #[serde(rename = "marketId")]
    pub market_id: i32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrderMatch {
    pub id: String,
    #[serde(rename = "createdAt", default)]
    pub created_at: Option<String>,
    #[serde(rename = "matchedSize")]
    pub matched_size: String,
    #[serde(rename = "orderId")]
    pub order_id: String,
}

/// Raw decimal totals for a settled or pending execution.
///
/// All six fields are decimal strings as returned by the API. They are not
/// coerced to numbers to preserve full precision.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct OrderExecutionTotalsRaw {
    #[serde(rename = "contractsGross", default)]
    pub contracts_gross: String,
    #[serde(rename = "contractsFee", default)]
    pub contracts_fee: String,
    #[serde(rename = "contractsNet", default)]
    pub contracts_net: String,
    #[serde(rename = "usdGross", default)]
    pub usd_gross: String,
    #[serde(rename = "usdFee", default)]
    pub usd_fee: String,
    #[serde(rename = "usdNet", default)]
    pub usd_net: String,
}

/// Execution outcome for a submitted order.
///
/// Always present on a successful `POST /orders` response. `settlement_status`
/// is a plain string (e.g. `DELAYED`, `MATCHED`, `CANCELED`, `MINED`,
/// `CONFIRMED`, `RETRYING`, `FAILED`, `UNMATCHED`) and is intentionally not
/// modeled as an enum so new server values do not break deserialization.
///
/// `reason` carries the self-trade prevention signal for a rejected taker
/// (e.g. `STP_TAKER_REJECTED`); it is HTTP-only. `stp_maker_cancels` lists the
/// canceled maker order ids when self-trade prevention canceled resting makers.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct OrderExecution {
    pub matched: bool,
    #[serde(rename = "settlementStatus", default)]
    pub settlement_status: String,
    #[serde(
        rename = "tradeEventId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub trade_event_id: Option<String>,
    #[serde(rename = "txHash", default, skip_serializing_if = "Option::is_none")]
    pub tx_hash: Option<String>,
    #[serde(
        rename = "clientOrderId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub client_order_id: Option<String>,
    #[serde(
        rename = "eligibleAt",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub eligible_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(
        rename = "stpMakerCancels",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub stp_maker_cancels: Option<Vec<String>>,
    #[serde(rename = "feeRateBps", default)]
    pub fee_rate_bps: f64,
    #[serde(rename = "effectiveFeeBps", default)]
    pub effective_fee_bps: f64,
    #[serde(rename = "totalsRaw", default)]
    pub totals_raw: OrderExecutionTotalsRaw,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrderResponse {
    pub order: CreatedOrder,
    #[serde(rename = "makerMatches", default)]
    pub maker_matches: Vec<OrderMatch>,
    /// Execution outcome of the submitted order.
    ///
    /// Always present on a live `POST /orders` response. Defaulted on
    /// deserialization to tolerate older API responses or hand-built fixtures
    /// that omit it.
    #[serde(default)]
    pub execution: OrderExecution,
}

#[derive(Clone, Debug, Default)]
pub struct OrderSigningConfig {
    pub chain_id: u64,
    pub contract_address: String,
}

#[derive(Clone, Debug)]
pub struct CreateOrderParams {
    pub order_type: OrderType,
    pub market_slug: String,
    pub args: OrderArgs,
    /// Optional self-trade prevention policy. Sent as the top-level `stpPolicy`
    /// request field, never part of the signed order. Omit to use the engine
    /// default (`cancel_maker`).
    pub stp_policy: Option<StpPolicy>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserData {
    pub user_id: i32,
    pub fee_rate_bps: i32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CancelResponse {
    pub message: String,
}

#[derive(Clone, Default)]
pub struct OrderClientOptions {
    pub signing_config: Option<OrderSigningConfig>,
    pub market_fetcher: Option<MarketFetcher>,
    pub portfolio_fetcher: Option<PortfolioFetcher>,
    pub logger: Option<SharedLogger>,
}

impl OrderClientOptions {
    #[must_use]
    pub fn with_signing_config(mut self, signing_config: OrderSigningConfig) -> Self {
        self.signing_config = Some(signing_config);
        self
    }

    #[must_use]
    pub fn with_market_fetcher(mut self, market_fetcher: MarketFetcher) -> Self {
        self.market_fetcher = Some(market_fetcher);
        self
    }

    #[must_use]
    pub fn with_portfolio_fetcher(mut self, portfolio_fetcher: PortfolioFetcher) -> Self {
        self.portfolio_fetcher = Some(portfolio_fetcher);
        self
    }

    #[must_use]
    pub fn with_logger(mut self, logger: SharedLogger) -> Self {
        self.logger = Some(logger);
        self
    }
}

pub struct OrderBuilder {
    maker_address: String,
    fee_rate_bps: i32,
    price_tick: f64,
}

impl OrderBuilder {
    pub fn new(
        maker_address: impl Into<String>,
        fee_rate_bps: i32,
        price_tick: Option<f64>,
    ) -> Self {
        Self {
            maker_address: maker_address.into(),
            fee_rate_bps,
            price_tick: price_tick.unwrap_or(DEFAULT_PRICE_TICK),
        }
    }

    pub fn build_order(&self, args: &OrderArgs) -> Result<UnsignedOrder> {
        validate_order_args_with_price_tick(args, self.price_tick)?;

        let (maker_amount, taker_amount, price) = match args {
            OrderArgs::Fok(fok) => {
                let (maker_amount, taker_amount) = self.calculate_fok_amounts(fok.maker_amount)?;
                (maker_amount, taker_amount, None)
            }
            OrderArgs::Gtc(gtc) => {
                let (maker_amount, taker_amount, price) =
                    self.calculate_limit_order_amounts(gtc.price, gtc.size, gtc.side)?;
                (maker_amount, taker_amount, Some(price))
            }
            OrderArgs::Fak(fak) => {
                let (maker_amount, taker_amount, price) =
                    self.calculate_limit_order_amounts(fak.price, fak.size, fak.side)?;
                (maker_amount, taker_amount, Some(price))
            }
        };

        let taker = match args {
            OrderArgs::Fok(value) => value
                .taker
                .clone()
                .unwrap_or_else(|| ZERO_ADDRESS.to_string()),
            OrderArgs::Gtc(value) => value
                .taker
                .clone()
                .unwrap_or_else(|| ZERO_ADDRESS.to_string()),
            OrderArgs::Fak(value) => value
                .taker
                .clone()
                .unwrap_or_else(|| ZERO_ADDRESS.to_string()),
        };

        let expiration = match args {
            OrderArgs::Fok(value) => value.expiration.clone().unwrap_or_else(|| "0".to_string()),
            OrderArgs::Gtc(value) => value.expiration.clone().unwrap_or_else(|| "0".to_string()),
            OrderArgs::Fak(value) => value.expiration.clone().unwrap_or_else(|| "0".to_string()),
        };

        let nonce = match args {
            OrderArgs::Fok(value) => value.nonce.unwrap_or(0),
            OrderArgs::Gtc(value) => value.nonce.unwrap_or(0),
            OrderArgs::Fak(value) => value.nonce.unwrap_or(0),
        };

        Ok(UnsignedOrder {
            salt: self.generate_salt(),
            maker: self.maker_address.clone(),
            signer: self.maker_address.clone(),
            taker,
            token_id: token_id_from_args(args).to_string(),
            maker_amount,
            taker_amount,
            expiration,
            nonce,
            fee_rate_bps: self.fee_rate_bps,
            side: side_from_args(args),
            signature_type: SignatureType::Eoa,
            price,
        })
    }

    fn generate_salt(&self) -> i64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_else(|_| Duration::from_millis(0));
        let candidate = i64::try_from(now.as_micros()).unwrap_or(i64::MAX - 1);

        loop {
            let previous = LAST_ORDER_SALT.load(Ordering::Relaxed);
            let next = candidate.max(previous.saturating_add(1));
            match LAST_ORDER_SALT.compare_exchange(
                previous,
                next,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => return next,
                Err(_) => continue,
            }
        }
    }

    fn calculate_fok_amounts(&self, maker_amount: f64) -> Result<(i64, i64)> {
        let amount_str = float_to_decimal_string(maker_amount);
        if decimal_places_from_str(&amount_str) > 6 {
            return Err(order_validation_error(
                "makerAmount",
                format!(
                    "invalid makerAmount: {maker_amount}. Can have max 6 decimal places. Try {:.6} instead",
                    maker_amount
                ),
            ));
        }

        let scaled = scale_to_6_decimals(maker_amount)?;
        Ok((scaled, 1))
    }

    fn calculate_limit_order_amounts(
        &self,
        price: f64,
        size: f64,
        side: Side,
    ) -> Result<(i64, i64, f64)> {
        let scale = scale6();
        let shares = parse_dec_to_int(&float_to_decimal_string(size), &scale);
        let price_int = parse_dec_to_int(&float_to_decimal_string(price), &scale);
        let tick_int = parse_dec_to_int(&float_to_decimal_string(self.price_tick), &scale);

        if tick_int <= BigInt::zero() {
            return Err(order_validation_error(
                "price",
                format!("invalid priceTick: {}", self.price_tick),
            ));
        }
        if price_int <= BigInt::zero() {
            return Err(order_validation_error(
                "price",
                format!("invalid price: {price}"),
            ));
        }
        if (&price_int % &tick_int) != BigInt::zero() {
            return Err(order_validation_error(
                "price",
                format!(
                    "price {price} is not tick-aligned. Must be multiple of {} (e.g., 0.380, 0.381, etc.)",
                    self.price_tick
                ),
            ));
        }

        let shares_step = &scale / &tick_int;
        if (&shares % &shares_step) != BigInt::zero() {
            let valid_down = (&shares / &shares_step) * &shares_step;
            let valid_up = div_ceil(&shares, &shares_step)? * &shares_step;
            return Err(order_validation_error(
                "size",
                format!(
                    "invalid size: {size}. Size must produce contracts divisible by {} (sharesStep). Try {} (rounded down) or {} (rounded up) instead",
                    shares_step,
                    format_scaled_bigint(&valid_down, 6),
                    format_scaled_bigint(&valid_up, 6)
                ),
            ));
        }

        let numerator = &shares * &price_int * &scale;
        let denominator = &scale * &scale;
        let collateral = if side == Side::Buy {
            div_ceil(&numerator, &denominator)?
        } else {
            numerator / denominator
        };

        let collateral_i64 = collateral.to_i64().ok_or_else(|| {
            order_validation_error(
                "makerAmount",
                format!("collateral overflow: value {collateral} exceeds i64 range"),
            )
        })?;
        let shares_i64 = shares.to_i64().ok_or_else(|| {
            order_validation_error(
                "size",
                format!("shares overflow: value {shares} exceeds i64 range"),
            )
        })?;

        let (maker_amount, taker_amount) = if side == Side::Buy {
            (collateral_i64, shares_i64)
        } else {
            (shares_i64, collateral_i64)
        };

        Ok((maker_amount, taker_amount, price))
    }
}

#[derive(Clone)]
pub struct OrderSigner {
    signing_key: SigningKey,
    address: String,
    logger: SharedLogger,
}

impl OrderSigner {
    pub fn new(private_key_hex: &str) -> Result<Self> {
        let hex_key = private_key_hex.trim_start_matches("0x");
        let private_key =
            Zeroizing::new(<[u8; 32]>::from_hex(hex_key).map_err(|err| {
                LimitlessError::invalid_input(format!("invalid private key: {err}"))
            })?);
        let signing_key = SigningKey::from_bytes((&*private_key).into())
            .map_err(|err| LimitlessError::invalid_input(format!("invalid private key: {err}")))?;

        let verifying_key = signing_key.verifying_key();
        let encoded = verifying_key.to_encoded_point(false);
        let pubkey = encoded.as_bytes();
        let digest = keccak256(&pubkey[1..]);
        let address = checksum_address(&digest[12..]);

        Ok(Self {
            signing_key,
            address,
            logger: noop_logger(),
        })
    }

    pub fn with_logger(mut self, logger: SharedLogger) -> Self {
        self.logger = logger;
        self
    }

    pub fn address(&self) -> &str {
        &self.address
    }

    pub fn sign_order(&self, order: &UnsignedOrder, config: &OrderSigningConfig) -> Result<String> {
        validate_signing_config(config)?;

        if !self.address.eq_ignore_ascii_case(&order.signer) {
            return Err(LimitlessError::invalid_input(format!(
                "wallet address mismatch! Signing with: {}, but order signer is: {}",
                self.address, order.signer
            )));
        }

        let domain_separator = hash_domain(config)?;
        let typed_data_hash = hash_order(order)?;

        let mut payload = Vec::with_capacity(66);
        payload.extend_from_slice(&[0x19, 0x01]);
        payload.extend_from_slice(&domain_separator);
        payload.extend_from_slice(&typed_data_hash);
        let message_hash = keccak256(&payload);

        let (signature, recovery_id): (Signature, RecoveryId) = self
            .signing_key
            .sign_prehash_recoverable(&message_hash)
            .map_err(|err| LimitlessError::Signing(format!("failed to sign order: {err}")))?;

        let mut bytes = signature.to_bytes().to_vec();
        bytes.push(recovery_id.to_byte() + 27);

        let encoded = format!("0x{}", hex::encode(bytes));
        self.logger.info("Successfully generated EIP-712 signature");
        Ok(encoded)
    }
}

#[derive(Clone)]
pub struct OrderClient {
    client: HttpClient,
    signer: OrderSigner,
    signing_config: OrderSigningConfig,
    market_fetcher: MarketFetcher,
    portfolio_fetcher: PortfolioFetcher,
    logger: SharedLogger,
    state: Arc<Mutex<OrderClientState>>,
    init_lock: Arc<tokio::sync::Mutex<()>>,
}

#[derive(Default)]
struct OrderClientState {
    user_data: Option<UserData>,
    builder: Option<OrderBuilder>,
}

impl OrderClient {
    pub fn new(
        client: HttpClient,
        private_key_hex: &str,
        options: Option<OrderClientOptions>,
    ) -> Result<Self> {
        let options = options.unwrap_or_default();
        let logger = options.logger.clone().unwrap_or_else(|| client.logger());
        let signer = OrderSigner::new(private_key_hex)?.with_logger(logger.clone());

        let chain_id = env::var("CHAIN_ID")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(DEFAULT_CHAIN_ID);
        let signing_config = options.signing_config.unwrap_or(OrderSigningConfig {
            chain_id,
            contract_address: String::new(),
        });

        Ok(Self {
            market_fetcher: options
                .market_fetcher
                .unwrap_or_else(|| MarketFetcher::new(client.clone())),
            portfolio_fetcher: options
                .portfolio_fetcher
                .unwrap_or_else(|| PortfolioFetcher::new(client.clone())),
            signer,
            signing_config,
            logger,
            state: Arc::new(Mutex::new(OrderClientState::default())),
            init_lock: Arc::new(tokio::sync::Mutex::new(())),
            client,
        })
    }

    pub async fn create_order(&self, params: CreateOrderParams) -> Result<OrderResponse> {
        self.create_order_internal(params, None).await
    }

    /// Creates an order with optional receive-window freshness controls.
    ///
    /// `timestamp` and `recv_window` are serialized as top-level `POST /orders`
    /// fields only. They are not part of the EIP-712 signed order payload.
    pub async fn create_order_with_receive_window(
        &self,
        params: CreateOrderParams,
        receive_window: ReceiveWindowOptions,
    ) -> Result<OrderResponse> {
        self.create_order_internal(params, Some(receive_window))
            .await
    }

    async fn create_order_internal(
        &self,
        params: CreateOrderParams,
        receive_window: Option<ReceiveWindowOptions>,
    ) -> Result<OrderResponse> {
        self.client.require_auth("CreateOrder")?;
        let receive_window = normalize_receive_window_options(receive_window, current_unix_ms)?;
        let user_data = self.ensure_user_data().await?;
        let signing_config = self
            .resolve_signing_config_for_market(&params.market_slug)
            .await?;

        let unsigned_order = self.build_unsigned_order(params.args.clone()).await?;
        let signature = self.signer.sign_order(&unsigned_order, &signing_config)?;

        let payload = NewOrderPayload {
            order: SignedOrder {
                salt: unsigned_order.salt,
                maker: unsigned_order.maker,
                signer: unsigned_order.signer,
                taker: unsigned_order.taker,
                token_id: unsigned_order.token_id,
                maker_amount: unsigned_order.maker_amount,
                taker_amount: unsigned_order.taker_amount,
                expiration: unsigned_order.expiration,
                nonce: unsigned_order.nonce,
                fee_rate_bps: unsigned_order.fee_rate_bps,
                side: unsigned_order.side,
                signature_type: unsigned_order.signature_type,
                price: unsigned_order.price,
                signature,
            },
            order_type: params.order_type,
            market_slug: params.market_slug,
            owner_id: user_data.user_id,
            post_only: post_only_from_args(&params.args),
            stp_policy: params.stp_policy,
            timestamp: receive_window.timestamp,
            recv_window: receive_window.recv_window,
        };

        self.client.post("/orders", &payload).await
    }

    pub async fn cancel(&self, order_id: &str) -> Result<String> {
        self.client.require_auth("Cancel")?;
        let response: CancelResponse = self
            .client
            .delete(&format!("/orders/{}", urlencoding::encode(order_id)))
            .await?;
        Ok(response.message)
    }

    pub async fn cancel_all(&self, market_slug: &str) -> Result<String> {
        self.client.require_auth("CancelAll")?;
        let response: CancelResponse = self
            .client
            .delete(&format!("/orders/all/{}", urlencoding::encode(market_slug)))
            .await?;
        Ok(response.message)
    }

    pub async fn build_unsigned_order(&self, args: OrderArgs) -> Result<UnsignedOrder> {
        self.ensure_user_data().await?;
        let state = self.state.lock().unwrap_or_else(|err| err.into_inner());
        let builder = state
            .builder
            .as_ref()
            .ok_or_else(|| LimitlessError::invalid_input("order builder is not initialized"))?;
        builder.build_order(&args)
    }

    pub fn sign_order(&self, order: &UnsignedOrder) -> Result<String> {
        validate_signing_config(&self.signing_config)?;
        self.signer.sign_order(order, &self.signing_config)
    }

    pub fn sign_order_with_config(
        &self,
        order: &UnsignedOrder,
        config: OrderSigningConfig,
    ) -> Result<String> {
        self.signer.sign_order(order, &config)
    }

    pub async fn sign_order_for_market(
        &self,
        market_slug: &str,
        order: &UnsignedOrder,
    ) -> Result<String> {
        let config = self.resolve_signing_config_for_market(market_slug).await?;
        self.signer.sign_order(order, &config)
    }

    pub fn wallet_address(&self) -> &str {
        self.signer.address()
    }

    pub fn owner_id(&self) -> Option<i32> {
        self.state
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .user_data
            .as_ref()
            .map(|data| data.user_id)
    }

    async fn ensure_user_data(&self) -> Result<UserData> {
        if let Some(user_data) = self
            .state
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .user_data
            .clone()
        {
            return Ok(user_data);
        }

        let _init_guard = self.init_lock.lock().await;
        if let Some(user_data) = self
            .state
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .user_data
            .clone()
        {
            return Ok(user_data);
        }

        self.client
            .require_auth("order creation and profile lookup")?;
        self.logger
            .info("Fetching user profile for order client initialization");

        let profile: UserProfile = self.portfolio_fetcher.get_current_profile().await?;
        let fee_rate_bps = profile
            .rank
            .as_ref()
            .map(|rank| rank.fee_rate_bps)
            .unwrap_or(DEFAULT_FEE_RATE_BPS);
        let user_data = UserData {
            user_id: profile.id,
            fee_rate_bps,
        };

        let mut state = self.state.lock().unwrap_or_else(|err| err.into_inner());
        state.builder = Some(OrderBuilder::new(
            self.signer.address().to_string(),
            fee_rate_bps,
            None,
        ));
        state.user_data = Some(user_data.clone());

        Ok(user_data)
    }

    async fn resolve_signing_config_for_market(
        &self,
        market_slug: &str,
    ) -> Result<OrderSigningConfig> {
        let venue = match self.market_fetcher.get_venue(market_slug) {
            Some(venue) if !venue.exchange.is_empty() => Some(venue),
            _ => {
                self.logger.warn(
                    "Venue not cached, fetching market details. For better performance, call get_market() before create_order().",
                );
                let market = self.market_fetcher.get_market(market_slug).await?;
                market.venue
            }
        };

        if let Some(Venue { exchange, .. }) = venue {
            if !exchange.is_empty() {
                let mut config = self.signing_config.clone();
                config.contract_address = exchange;
                return Ok(config);
            }
        }

        if validate_signing_config(&self.signing_config).is_ok() {
            self.logger.warn(
                "Market venue is missing an exchange contract; using fallback signing config",
            );
            return Ok(self.signing_config.clone());
        }

        Err(LimitlessError::invalid_input(format!(
            "market {market_slug} does not expose venue.exchange and no fallback signing contract is configured"
        )))
    }
}

pub(crate) fn normalize_receive_window_options(
    options: Option<ReceiveWindowOptions>,
    now_ms: impl FnOnce() -> i64,
) -> Result<ReceiveWindowOptions> {
    let mut normalized = options.unwrap_or_default();

    if matches!(normalized.timestamp, Some(timestamp) if timestamp < 0) {
        return Err(LimitlessError::invalid_input(
            "timestamp must be a non-negative integer",
        ));
    }

    if let Some(recv_window) = normalized.recv_window {
        if !(1..=MAX_RECV_WINDOW_MS).contains(&recv_window) {
            return Err(LimitlessError::invalid_input(format!(
                "recv_window must be between 1 and {MAX_RECV_WINDOW_MS} milliseconds"
            )));
        }
    }

    if normalized.recv_window.is_some() && normalized.timestamp.is_none() {
        normalized.timestamp = Some(now_ms());
    }

    Ok(normalized)
}

pub(crate) fn current_unix_ms() -> i64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_millis(0));
    i64::try_from(now.as_millis()).unwrap_or(i64::MAX)
}

pub fn validate_order_args(args: &OrderArgs) -> Result<()> {
    validate_order_args_with_price_tick(args, DEFAULT_PRICE_TICK)
}

pub fn validate_order_args_with_price_tick(args: &OrderArgs, price_tick: f64) -> Result<()> {
    match args {
        OrderArgs::Fok(value) => {
            validate_token_id(&value.token_id)?;
            if value.maker_amount <= 0.0 {
                return Err(order_validation_error(
                    "makerAmount",
                    format!("amount must be positive, got: {}", value.maker_amount),
                ));
            }
            let amount_text = float_to_decimal_string(value.maker_amount);
            if decimal_places_from_str(&amount_text) > 6 {
                return Err(order_validation_error(
                    "makerAmount",
                    format!(
                        "amount must have max 6 decimal places, got: {} ({} decimals)",
                        value.maker_amount,
                        decimal_places_from_str(&amount_text)
                    ),
                ));
            }
            validate_optional_order_fields(
                value.taker.as_deref(),
                value.expiration.as_deref(),
                value.nonce,
            )?;
        }
        OrderArgs::Gtc(value) => {
            validate_limit_order_args(
                &value.token_id,
                value.price,
                value.size,
                value.expiration.as_deref(),
                value.nonce,
                value.taker.as_deref(),
                price_tick,
            )?;
        }
        OrderArgs::Fak(value) => {
            validate_limit_order_args(
                &value.token_id,
                value.price,
                value.size,
                value.expiration.as_deref(),
                value.nonce,
                value.taker.as_deref(),
                price_tick,
            )?;
        }
    }
    Ok(())
}

pub fn validate_unsigned_order(order: &UnsignedOrder) -> Result<()> {
    if !is_valid_address(&order.maker) {
        return Err(order_validation_error(
            "maker",
            format!("invalid maker address: {}", order.maker),
        ));
    }
    if !is_valid_address(&order.signer) {
        return Err(order_validation_error(
            "signer",
            format!("invalid signer address: {}", order.signer),
        ));
    }
    if !is_valid_address(&order.taker) {
        return Err(order_validation_error(
            "taker",
            format!("invalid taker address: {}", order.taker),
        ));
    }
    if order.maker_amount <= 0 {
        return Err(order_validation_error(
            "makerAmount",
            "makerAmount must be greater than zero".to_string(),
        ));
    }
    if order.taker_amount <= 0 {
        return Err(order_validation_error(
            "takerAmount",
            "takerAmount must be greater than zero".to_string(),
        ));
    }
    if !NUMERIC_REGEX.is_match(&order.token_id) {
        return Err(order_validation_error(
            "tokenId",
            format!("invalid tokenId format: {}", order.token_id),
        ));
    }
    if !NUMERIC_REGEX.is_match(&order.expiration) {
        return Err(order_validation_error(
            "expiration",
            format!("invalid expiration format: {}", order.expiration),
        ));
    }
    if order.salt <= 0 {
        return Err(order_validation_error(
            "salt",
            format!("invalid salt: {}", order.salt),
        ));
    }
    if order.nonce < 0 {
        return Err(order_validation_error(
            "nonce",
            format!("invalid nonce: {}", order.nonce),
        ));
    }
    if order.fee_rate_bps < 0 {
        return Err(order_validation_error(
            "feeRateBps",
            format!("invalid feeRateBps: {}", order.fee_rate_bps),
        ));
    }
    if order.side != Side::Buy && order.side != Side::Sell {
        return Err(order_validation_error(
            "side",
            format!("invalid side: {}", order.side as u8),
        ));
    }
    if let Some(price) = order.price {
        if !(0.0..=1.0).contains(&price) || price == 0.0 {
            return Err(order_validation_error(
                "price",
                format!("price must be between 0 and 1, got: {price}"),
            ));
        }
    }
    Ok(())
}

pub fn validate_signed_order(order: &SignedOrder) -> Result<()> {
    validate_unsigned_order(&UnsignedOrder {
        salt: order.salt,
        maker: order.maker.clone(),
        signer: order.signer.clone(),
        taker: order.taker.clone(),
        token_id: order.token_id.clone(),
        maker_amount: order.maker_amount,
        taker_amount: order.taker_amount,
        expiration: order.expiration.clone(),
        nonce: order.nonce,
        fee_rate_bps: order.fee_rate_bps,
        side: order.side,
        signature_type: order.signature_type,
        price: order.price,
    })?;

    if order.signature.trim().is_empty() {
        return Err(order_validation_error(
            "signature",
            "signature is required".to_string(),
        ));
    }
    if !SIGNATURE_REGEX.is_match(&order.signature) {
        return Err(order_validation_error(
            "signature",
            format!("invalid signature format: {}", order.signature),
        ));
    }

    Ok(())
}

pub(crate) fn post_only_from_args(args: &OrderArgs) -> Option<bool> {
    match args {
        OrderArgs::Gtc(value) if value.post_only => Some(true),
        _ => None,
    }
}

pub(crate) fn token_id_from_args(args: &OrderArgs) -> &str {
    match args {
        OrderArgs::Fok(value) => &value.token_id,
        OrderArgs::Gtc(value) => &value.token_id,
        OrderArgs::Fak(value) => &value.token_id,
    }
}

fn side_from_args(args: &OrderArgs) -> Side {
    match args {
        OrderArgs::Fok(value) => value.side,
        OrderArgs::Gtc(value) => value.side,
        OrderArgs::Fak(value) => value.side,
    }
}

fn validate_limit_order_args(
    token_id: &str,
    price: f64,
    size: f64,
    expiration: Option<&str>,
    nonce: Option<i32>,
    taker: Option<&str>,
    price_tick: f64,
) -> Result<()> {
    validate_token_id(token_id)?;
    if !(0.0..=1.0).contains(&price) || price == 0.0 {
        return Err(order_validation_error(
            "price",
            format!("price must be between 0 and 1, got: {price}"),
        ));
    }
    if size <= 0.0 {
        return Err(order_validation_error(
            "size",
            format!("size must be positive, got: {size}"),
        ));
    }

    let max_price_decimals = decimal_places_from_str(&float_to_decimal_string(price_tick));
    let price_text = float_to_decimal_string(price);
    if decimal_places_from_str(&price_text) > max_price_decimals {
        return Err(order_validation_error(
            "price",
            format!(
                "price must have max {max_price_decimals} decimal places, got: {price} ({} decimals)",
                decimal_places_from_str(&price_text)
            ),
        ));
    }

    let size_text = float_to_decimal_string(size);
    if decimal_places_from_str(&size_text) > 6 {
        return Err(order_validation_error(
            "size",
            format!(
                "size must have max 6 decimal places, got: {size} ({} decimals)",
                decimal_places_from_str(&size_text)
            ),
        ));
    }

    let scale = scale6();
    let price_int = parse_dec_to_int(&price_text, &scale);
    let tick_int = parse_dec_to_int(&float_to_decimal_string(price_tick), &scale);
    if tick_int <= BigInt::zero() {
        return Err(order_validation_error(
            "price",
            format!("invalid priceTick: {price_tick}"),
        ));
    }
    if (&price_int % &tick_int) != BigInt::zero() {
        return Err(order_validation_error(
            "price",
            format!(
                "price {price} is not tick-aligned. Must be multiple of {price_tick} (e.g., 0.380, 0.381, etc.)"
            ),
        ));
    }

    let shares = parse_dec_to_int(&size_text, &scale);
    let shares_step = &scale / &tick_int;
    if (&shares % &shares_step) != BigInt::zero() {
        let valid_down = (&shares / &shares_step) * &shares_step;
        let valid_up = div_ceil(&shares, &shares_step)? * &shares_step;
        return Err(order_validation_error(
            "size",
            format!(
                "invalid size: {size}. Size must produce contracts divisible by {} (sharesStep). Try {} (rounded down) or {} (rounded up) instead",
                shares_step,
                format_scaled_bigint(&valid_down, 6),
                format_scaled_bigint(&valid_up, 6)
            ),
        ));
    }

    validate_optional_order_fields(taker, expiration, nonce)
}

fn validate_token_id(token_id: &str) -> Result<()> {
    if token_id.is_empty() {
        return Err(order_validation_error(
            "tokenId",
            "tokenId is required".to_string(),
        ));
    }
    if token_id == "0" {
        return Err(order_validation_error(
            "tokenId",
            "tokenId cannot be zero".to_string(),
        ));
    }
    if !NUMERIC_REGEX.is_match(token_id) {
        return Err(order_validation_error(
            "tokenId",
            format!("invalid tokenId format: {token_id}"),
        ));
    }
    Ok(())
}

fn validate_optional_order_fields(
    taker: Option<&str>,
    expiration: Option<&str>,
    nonce: Option<i32>,
) -> Result<()> {
    if let Some(taker) = taker {
        if !taker.is_empty() && !is_valid_address(taker) {
            return Err(order_validation_error(
                "taker",
                format!("invalid taker address: {taker}"),
            ));
        }
    }
    if let Some(expiration) = expiration {
        if !expiration.is_empty() && !NUMERIC_REGEX.is_match(expiration) {
            return Err(order_validation_error(
                "expiration",
                format!("invalid expiration format: {expiration}"),
            ));
        }
    }
    if let Some(nonce) = nonce {
        if nonce < 0 {
            return Err(order_validation_error(
                "nonce",
                format!("invalid nonce: {nonce}"),
            ));
        }
    }
    Ok(())
}

fn validate_signing_config(config: &OrderSigningConfig) -> Result<()> {
    if config.chain_id == 0 {
        return Err(LimitlessError::invalid_input("invalid chain ID 0"));
    }
    if config.contract_address.trim().is_empty() {
        return Err(LimitlessError::invalid_input(
            "verifying contract address is required",
        ));
    }
    if config.contract_address.eq_ignore_ascii_case(ZERO_ADDRESS) {
        return Err(LimitlessError::invalid_input(
            "verifying contract address must not be the zero address",
        ));
    }
    if !is_valid_address(&config.contract_address) {
        return Err(LimitlessError::invalid_input(format!(
            "invalid verifying contract address: {}",
            config.contract_address
        )));
    }
    Ok(())
}

fn hash_domain(config: &OrderSigningConfig) -> Result<[u8; 32]> {
    let mut encoded = Vec::with_capacity(32 * 5);
    encoded.extend_from_slice(&*DOMAIN_TYPEHASH);
    encoded.extend_from_slice(&*DOMAIN_NAME_HASH);
    encoded.extend_from_slice(&*DOMAIN_VERSION_HASH);
    encoded.extend_from_slice(&encode_u256_from_u64(config.chain_id));
    encoded.extend_from_slice(&encode_address(&config.contract_address)?);
    Ok(keccak256(&encoded))
}

fn hash_order(order: &UnsignedOrder) -> Result<[u8; 32]> {
    validate_unsigned_order(order)?;

    let mut encoded = Vec::with_capacity(32 * 13);
    encoded.extend_from_slice(&*ORDER_TYPEHASH);
    encoded.extend_from_slice(&encode_u256_from_i64(order.salt)?);
    encoded.extend_from_slice(&encode_address(&order.maker)?);
    encoded.extend_from_slice(&encode_address(&order.signer)?);
    encoded.extend_from_slice(&encode_address(&order.taker)?);
    encoded.extend_from_slice(&encode_u256_from_decimal(&order.token_id)?);
    encoded.extend_from_slice(&encode_u256_from_i64(order.maker_amount)?);
    encoded.extend_from_slice(&encode_u256_from_i64(order.taker_amount)?);
    encoded.extend_from_slice(&encode_u256_from_decimal(&order.expiration)?);
    encoded.extend_from_slice(&encode_u256_from_i32(order.nonce)?);
    encoded.extend_from_slice(&encode_u256_from_i32(order.fee_rate_bps)?);
    encoded.extend_from_slice(&encode_u256_from_u64(order.side as u64));
    encoded.extend_from_slice(&encode_u256_from_u64(order.signature_type as u64));
    Ok(keccak256(&encoded))
}

fn encode_u256_from_i64(value: i64) -> Result<[u8; 32]> {
    if value < 0 {
        return Err(LimitlessError::invalid_input(format!(
            "expected non-negative integer, got {value}"
        )));
    }
    encode_bigint_to_u256(&BigInt::from(value))
}

fn encode_u256_from_i32(value: i32) -> Result<[u8; 32]> {
    if value < 0 {
        return Err(LimitlessError::invalid_input(format!(
            "expected non-negative integer, got {value}"
        )));
    }
    encode_bigint_to_u256(&BigInt::from(value))
}

fn encode_u256_from_u64(value: u64) -> [u8; 32] {
    let mut out = [0_u8; 32];
    out[24..].copy_from_slice(&value.to_be_bytes());
    out
}

fn encode_u256_from_decimal(value: &str) -> Result<[u8; 32]> {
    if !NUMERIC_REGEX.is_match(value) {
        return Err(LimitlessError::invalid_input(format!(
            "invalid uint value: {value}"
        )));
    }
    let bigint = BigInt::parse_bytes(value.as_bytes(), 10)
        .ok_or_else(|| LimitlessError::invalid_input(format!("invalid uint value: {value}")))?;
    encode_bigint_to_u256(&bigint)
}

fn encode_bigint_to_u256(value: &BigInt) -> Result<[u8; 32]> {
    if value.sign() == Sign::Minus {
        return Err(LimitlessError::invalid_input("expected unsigned integer"));
    }

    let (_, bytes) = value.to_bytes_be();
    if bytes.len() > 32 {
        return Err(LimitlessError::invalid_input(format!(
            "value {} exceeds uint256 size",
            value
        )));
    }

    let mut out = [0_u8; 32];
    let start = 32 - bytes.len();
    out[start..].copy_from_slice(&bytes);
    Ok(out)
}

fn encode_address(address: &str) -> Result<[u8; 32]> {
    let raw = parse_address(address)?;
    let mut out = [0_u8; 32];
    out[12..].copy_from_slice(&raw);
    Ok(out)
}

fn parse_address(address: &str) -> Result<[u8; 20]> {
    if !is_valid_address(address) {
        return Err(LimitlessError::invalid_input(format!(
            "invalid address: {address}"
        )));
    }
    let bytes = hex::decode(&address[2..])
        .map_err(|err| LimitlessError::invalid_input(format!("invalid address: {err}")))?;
    let raw: [u8; 20] = bytes
        .try_into()
        .map_err(|_| LimitlessError::invalid_input(format!("invalid address: {address}")))?;
    Ok(raw)
}

fn checksum_address(bytes: &[u8]) -> String {
    let lower = hex::encode(bytes);
    let hash = keccak256(lower.as_bytes());
    let mut out = String::with_capacity(42);
    out.push_str("0x");
    for (index, ch) in lower.chars().enumerate() {
        let nibble = if index % 2 == 0 {
            (hash[index / 2] >> 4) & 0x0f
        } else {
            hash[index / 2] & 0x0f
        };
        if ch.is_ascii_alphabetic() && nibble >= 8 {
            out.push(ch.to_ascii_uppercase());
        } else {
            out.push(ch);
        }
    }
    out
}

fn order_validation_error(field: &str, message: String) -> LimitlessError {
    LimitlessError::invalid_input(format!("order validation error [{field}]: {message}"))
}

fn scale6() -> BigInt {
    BigInt::from(1_000_000_i64)
}

fn parse_dec_to_int(value: &str, scale: &BigInt) -> BigInt {
    let trimmed = value.trim();
    let mut parts = trimmed.splitn(2, '.');
    let mut int_part = parts.next().unwrap_or("0");
    let mut frac_part = parts.next().unwrap_or("").to_string();

    let decimals = scale.to_string().len().saturating_sub(1);
    if frac_part.len() < decimals {
        frac_part.push_str(&"0".repeat(decimals - frac_part.len()));
    } else if frac_part.len() > decimals {
        frac_part.truncate(decimals);
    }

    let negative = int_part.starts_with('-');
    if negative {
        int_part = int_part.trim_start_matches('-');
    }
    if int_part.is_empty() {
        int_part = "0";
    }

    let int_val = BigInt::parse_bytes(int_part.as_bytes(), 10).unwrap_or_else(BigInt::zero);
    let frac_val = if frac_part.is_empty() {
        BigInt::zero()
    } else {
        BigInt::parse_bytes(frac_part.as_bytes(), 10).unwrap_or_else(BigInt::zero)
    };

    let mut result = int_val * scale + frac_val;
    if negative {
        result = -result;
    }
    result
}

fn div_ceil(a: &BigInt, b: &BigInt) -> Result<BigInt> {
    if b.is_zero() {
        return Err(LimitlessError::invalid_input("division by zero"));
    }

    let quotient = a / b;
    let remainder = a % b;
    if remainder.is_zero() {
        return Ok(quotient);
    }

    let same_sign = a.sign() == b.sign();
    if same_sign {
        Ok(quotient + 1)
    } else {
        Ok(quotient)
    }
}

fn scale_to_6_decimals(amount: f64) -> Result<i64> {
    let result = parse_dec_to_int(&format!("{amount:.6}"), &scale6());
    result.to_i64().ok_or_else(|| {
        LimitlessError::invalid_input(format!(
            "overflow: scaled value {} exceeds i64 range",
            result
        ))
    })
}

fn is_valid_address(address: &str) -> bool {
    address.len() == 42
        && address.starts_with("0x")
        && address[2..].chars().all(|ch| ch.is_ascii_hexdigit())
}

fn float_to_decimal_string(value: f64) -> String {
    let mut formatted = format!("{value:.12}");
    while formatted.contains('.') && formatted.ends_with('0') {
        formatted.pop();
    }
    if formatted.ends_with('.') {
        formatted.pop();
    }
    if formatted == "-0" {
        "0".to_string()
    } else {
        formatted
    }
}

fn decimal_places_from_str(value: &str) -> usize {
    value.split('.').nth(1).map(str::len).unwrap_or(0)
}

fn format_scaled_bigint(value: &BigInt, decimals: usize) -> String {
    let scale = BigInt::from(10_u64).pow(decimals as u32);
    let negative = value.sign() == Sign::Minus;
    let abs = value.abs();
    let int_part = &abs / &scale;
    let frac_part = &abs % &scale;

    if frac_part.is_zero() {
        return if negative {
            format!("-{int_part}")
        } else {
            int_part.to_string()
        };
    }

    let mut frac = frac_part.to_string();
    if frac.len() < decimals {
        frac = format!("{}{}", "0".repeat(decimals - frac.len()), frac);
    }
    while frac.ends_with('0') {
        frac.pop();
    }

    if negative {
        format!("-{int_part}.{frac}")
    } else {
        format!("{int_part}.{frac}")
    }
}

fn keccak256(data: impl AsRef<[u8]>) -> [u8; 32] {
    let mut hasher = Keccak256::new();
    hasher.update(data.as_ref());
    hasher.finalize().into()
}

fn deserialize_i64_from_number_or_string<'de, D>(
    deserializer: D,
) -> std::result::Result<i64, D::Error>
where
    D: Deserializer<'de>,
{
    match serde_json::Value::deserialize(deserializer)? {
        serde_json::Value::Number(number) => number
            .as_i64()
            .ok_or_else(|| D::Error::custom("expected i64-compatible number")),
        serde_json::Value::String(value) => value
            .parse::<i64>()
            .map_err(|err| D::Error::custom(format!("invalid i64 string: {err}"))),
        other => Err(D::Error::custom(format!(
            "expected number or numeric string, got {other}"
        ))),
    }
}

fn deserialize_option_f64_from_number_or_string<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<f64>, D::Error>
where
    D: Deserializer<'de>,
{
    match Option::<serde_json::Value>::deserialize(deserializer)? {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::Number(number)) => number
            .as_f64()
            .ok_or_else(|| D::Error::custom("expected f64-compatible number"))
            .map(Some),
        Some(serde_json::Value::String(value)) => value
            .parse::<f64>()
            .map(Some)
            .map_err(|err| D::Error::custom(format!("invalid f64 string: {err}"))),
        Some(other) => Err(D::Error::custom(format!(
            "expected number, numeric string, or null, got {other}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const TEST_PRIVATE_KEY_HEX: &str =
        "0x59c6995e998f97a5a0044966f0945382d7f84be58f4d1e8e8f8d0f9f5c5e7d5a";
    const TEST_SIGNER_ADDRESS: &str = "0xa00BCB04073B243E8A55f3B5899AefF596bF17C6";
    const EXPECTED_ORDER_SIGNATURE: &str =
        "0x0cd29c83ce6390a8adba01ae3d49a0dbf875bd8d06902f6381676a11159afbcf22c35f5acaea3aca85152f254851e548946fe56f68f793ced0b08c2626729adc1c";

    fn test_unsigned_order_for_signer() -> UnsignedOrder {
        UnsignedOrder {
            salt: 1_742_191_300_000_000,
            maker: TEST_SIGNER_ADDRESS.to_string(),
            signer: TEST_SIGNER_ADDRESS.to_string(),
            taker: ZERO_ADDRESS.to_string(),
            token_id: "12345".to_string(),
            maker_amount: 470_154,
            taker_amount: 1_234_000,
            expiration: "0".to_string(),
            nonce: 0,
            fee_rate_bps: 300,
            side: Side::Buy,
            signature_type: SignatureType::Eoa,
            price: Some(0.381),
        }
    }

    fn test_signed_order() -> SignedOrder {
        let order = test_unsigned_order_for_signer();
        SignedOrder {
            salt: order.salt,
            maker: order.maker,
            signer: order.signer,
            taker: order.taker,
            token_id: order.token_id,
            maker_amount: order.maker_amount,
            taker_amount: order.taker_amount,
            expiration: order.expiration,
            nonce: order.nonce,
            fee_rate_bps: order.fee_rate_bps,
            side: order.side,
            signature_type: order.signature_type,
            price: order.price,
            signature: EXPECTED_ORDER_SIGNATURE.to_string(),
        }
    }

    fn test_create_order_params() -> CreateOrderParams {
        CreateOrderParams {
            order_type: OrderType::Gtc,
            market_slug: "test-market".to_string(),
            args: OrderArgs::Gtc(GtcOrderArgs {
                token_id: "12345".to_string(),
                side: Side::Buy,
                price: 0.381,
                size: 1.234,
                expiration: None,
                nonce: None,
                taker: None,
                post_only: false,
            }),
            stp_policy: None,
        }
    }

    #[test]
    fn receive_window_normalization_omits_defaults_and_auto_stamps() {
        let empty = normalize_receive_window_options(None, || {
            panic!("clock should not be read without recv_window")
        })
        .expect("empty receive-window options should normalize");
        assert_eq!(empty, ReceiveWindowOptions::default());

        let timestamp_only = normalize_receive_window_options(
            Some(ReceiveWindowOptions {
                timestamp: Some(0),
                recv_window: None,
            }),
            || 1_770_000_000_000,
        )
        .expect("timestamp-only receive-window options should normalize");
        assert_eq!(timestamp_only.timestamp, Some(0));
        assert_eq!(timestamp_only.recv_window, None);

        let stamped = normalize_receive_window_options(
            Some(ReceiveWindowOptions {
                timestamp: None,
                recv_window: Some(1500),
            }),
            || 1_770_000_000_000,
        )
        .expect("recv_window-only receive-window options should normalize");
        assert_eq!(stamped.timestamp, Some(1_770_000_000_000));
        assert_eq!(stamped.recv_window, Some(1500));
    }

    #[test]
    fn receive_window_normalization_rejects_invalid_values() {
        for options in [
            ReceiveWindowOptions {
                timestamp: Some(-1),
                recv_window: None,
            },
            ReceiveWindowOptions {
                timestamp: None,
                recv_window: Some(0),
            },
            ReceiveWindowOptions {
                timestamp: None,
                recv_window: Some(10_001),
            },
        ] {
            let error = normalize_receive_window_options(Some(options), || 1_770_000_000_000)
                .expect_err("invalid receive-window options should fail");
            assert!(matches!(error, LimitlessError::InvalidInput(_)));
        }
    }

    #[test]
    fn new_order_payload_serializes_receive_window_top_level_only() {
        let payload = NewOrderPayload {
            order: test_signed_order(),
            order_type: OrderType::Gtc,
            market_slug: "test-market".to_string(),
            owner_id: 42,
            post_only: None,
            stp_policy: None,
            timestamp: Some(1_770_000_000_000),
            recv_window: Some(1500),
        };

        let value = serde_json::to_value(&payload).expect("payload should serialize");
        assert_eq!(value["timestamp"], json!(1_770_000_000_000_i64));
        assert_eq!(value["recvWindow"], json!(1500));
        assert!(value["order"].get("timestamp").is_none());
        assert!(value["order"].get("recvWindow").is_none());
    }

    #[test]
    fn new_order_payload_omits_receive_window_by_default() {
        let payload = NewOrderPayload {
            order: test_signed_order(),
            order_type: OrderType::Gtc,
            market_slug: "test-market".to_string(),
            owner_id: 42,
            post_only: None,
            stp_policy: None,
            timestamp: None,
            recv_window: None,
        };

        let value = serde_json::to_value(&payload).expect("payload should serialize");
        assert!(value.get("timestamp").is_none());
        assert!(value.get("recvWindow").is_none());
        assert!(value["order"].get("timestamp").is_none());
        assert!(value["order"].get("recvWindow").is_none());
    }

    #[test]
    fn new_order_payload_serializes_stp_policy_top_level_only() {
        let payload = NewOrderPayload {
            order: test_signed_order(),
            order_type: OrderType::Gtc,
            market_slug: "test-market".to_string(),
            owner_id: 42,
            post_only: None,
            stp_policy: Some(StpPolicy::CancelBoth),
            timestamp: None,
            recv_window: None,
        };

        let value = serde_json::to_value(&payload).expect("payload should serialize");
        assert_eq!(value["stpPolicy"], json!("cancel_both"));
        assert!(value["order"].get("stpPolicy").is_none());
        // Signed order stays exactly 12 fields plus the signature.
        let order = value["order"].as_object().expect("order object");
        assert_eq!(order.len(), 14);
    }

    #[test]
    fn new_order_payload_omits_stp_policy_by_default() {
        let payload = NewOrderPayload {
            order: test_signed_order(),
            order_type: OrderType::Gtc,
            market_slug: "test-market".to_string(),
            owner_id: 42,
            post_only: None,
            stp_policy: None,
            timestamp: None,
            recv_window: None,
        };

        let value = serde_json::to_value(&payload).expect("payload should serialize");
        assert!(value.get("stpPolicy").is_none());
    }

    #[test]
    fn stp_policy_values_are_snake_case() {
        assert_eq!(
            serde_json::to_value(StpPolicy::CancelMaker).unwrap(),
            json!("cancel_maker")
        );
        assert_eq!(
            serde_json::to_value(StpPolicy::CancelTaker).unwrap(),
            json!("cancel_taker")
        );
        assert_eq!(
            serde_json::from_value::<StpPolicy>(json!("cancel_both")).unwrap(),
            StpPolicy::CancelBoth
        );
    }

    #[test]
    fn order_response_deserializes_execution_with_stp_signals() {
        let response: OrderResponse = serde_json::from_value(json!({
            "order": {
                "id": "order-1",
                "createdAt": "2026-06-08T00:00:00.000Z",
                "makerAmount": 470154,
                "takerAmount": 1234000,
                "signatureType": 0,
                "salt": 1742191300000000_i64,
                "maker": TEST_SIGNER_ADDRESS,
                "signer": TEST_SIGNER_ADDRESS,
                "taker": ZERO_ADDRESS,
                "tokenId": "12345",
                "side": 0,
                "feeRateBps": 300,
                "nonce": 0,
                "signature": EXPECTED_ORDER_SIGNATURE,
                "orderType": "GTC",
                "price": 0.381,
                "marketId": 7
            },
            "makerMatches": [],
            "execution": {
                "matched": false,
                "settlementStatus": "CANCELED",
                "reason": "STP_TAKER_REJECTED",
                "stpMakerCancels": ["maker-a", "maker-b"],
                "feeRateBps": 300,
                "effectiveFeeBps": 0,
                "totalsRaw": {
                    "contractsGross": "0",
                    "contractsFee": "0",
                    "contractsNet": "0",
                    "usdGross": "0",
                    "usdFee": "0",
                    "usdNet": "0"
                }
            }
        }))
        .expect("order response should deserialize");

        assert!(!response.execution.matched);
        assert_eq!(response.execution.settlement_status, "CANCELED");
        assert_eq!(
            response.execution.reason.as_deref(),
            Some("STP_TAKER_REJECTED")
        );
        assert_eq!(
            response.execution.stp_maker_cancels,
            Some(vec!["maker-a".to_string(), "maker-b".to_string()])
        );
        assert_eq!(response.execution.fee_rate_bps, 300.0);
        assert_eq!(response.execution.effective_fee_bps, 0.0);
        assert_eq!(response.execution.totals_raw.usd_net, "0");
    }

    #[test]
    fn order_response_tolerates_missing_execution() {
        let response: OrderResponse = serde_json::from_value(json!({
            "order": {
                "id": "order-1",
                "createdAt": "2026-06-08T00:00:00.000Z",
                "makerAmount": 470154,
                "takerAmount": 1234000,
                "signatureType": 0,
                "salt": 1742191300000000_i64,
                "maker": TEST_SIGNER_ADDRESS,
                "signer": TEST_SIGNER_ADDRESS,
                "taker": ZERO_ADDRESS,
                "tokenId": "12345",
                "side": 0,
                "feeRateBps": 300,
                "nonce": 0,
                "signature": EXPECTED_ORDER_SIGNATURE,
                "orderType": "GTC",
                "price": 0.381,
                "marketId": 7
            }
        }))
        .expect("order response without execution should deserialize");

        assert!(!response.execution.matched);
        assert_eq!(response.execution.settlement_status, "");
        assert!(response.execution.reason.is_none());
        assert!(response.execution.stp_maker_cancels.is_none());
    }

    #[test]
    fn create_order_with_receive_window_rejects_invalid_values_before_network() {
        let client = HttpClient::builder()
            .api_key("test-api-key")
            .build()
            .expect("client");
        let order_client =
            OrderClient::new(client, TEST_PRIVATE_KEY_HEX, None).expect("order client");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        let error = runtime
            .block_on(order_client.create_order_with_receive_window(
                test_create_order_params(),
                ReceiveWindowOptions {
                    timestamp: None,
                    recv_window: Some(0),
                },
            ))
            .expect_err("invalid receive-window options should fail");

        assert!(matches!(error, LimitlessError::InvalidInput(_)));
        assert!(error.to_string().contains("recv_window"));
    }

    #[test]
    fn order_builder_fok_scales_amounts() {
        let builder = OrderBuilder::new(TEST_SIGNER_ADDRESS, 300, None);
        let order = builder
            .build_order(&OrderArgs::Fok(FokOrderArgs {
                token_id: "12345".to_string(),
                side: Side::Buy,
                maker_amount: 12.345678,
                expiration: None,
                nonce: None,
                taker: None,
            }))
            .expect("FOK order should build");

        assert_eq!(order.maker_amount, 12_345_678);
        assert_eq!(order.taker_amount, 1);
        assert_eq!(order.taker, ZERO_ADDRESS);
        assert_eq!(order.expiration, "0");
        assert_eq!(order.nonce, 0);
        assert!(order.price.is_none());
    }

    #[test]
    fn order_builder_gtc_buy_amounts_match_go_vector() {
        let builder = OrderBuilder::new(TEST_SIGNER_ADDRESS, 300, None);
        let order = builder
            .build_order(&OrderArgs::Gtc(GtcOrderArgs {
                token_id: "12345".to_string(),
                side: Side::Buy,
                price: 0.381,
                size: 1.234,
                expiration: None,
                nonce: None,
                taker: None,
                post_only: false,
            }))
            .expect("GTC order should build");

        assert_eq!(order.maker_amount, 470_154);
        assert_eq!(order.taker_amount, 1_234_000);
        assert_eq!(order.price, Some(0.381));
    }

    #[test]
    fn order_match_deserializes_without_created_at() {
        let order_match: OrderMatch = serde_json::from_value(json!({
            "id": "match-1",
            "matchedSize": "100",
            "orderId": "order-1"
        }))
        .expect("order match should deserialize");

        assert_eq!(order_match.id, "match-1");
        assert_eq!(order_match.created_at, None);
        assert_eq!(order_match.matched_size, "100");
        assert_eq!(order_match.order_id, "order-1");
    }

    #[test]
    fn order_match_deserializes_with_null_created_at() {
        let order_match: OrderMatch = serde_json::from_value(json!({
            "id": "match-1",
            "createdAt": null,
            "matchedSize": "100",
            "orderId": "order-1"
        }))
        .expect("order match should deserialize");

        assert_eq!(order_match.id, "match-1");
        assert_eq!(order_match.created_at, None);
        assert_eq!(order_match.matched_size, "100");
        assert_eq!(order_match.order_id, "order-1");
    }

    #[test]
    fn order_builder_uses_custom_price_tick_validation() {
        let builder = OrderBuilder::new(TEST_SIGNER_ADDRESS, 300, Some(0.0001));
        let order = builder
            .build_order(&OrderArgs::Gtc(GtcOrderArgs {
                token_id: "12345".to_string(),
                side: Side::Buy,
                price: 0.3815,
                size: 1.23,
                expiration: None,
                nonce: None,
                taker: None,
                post_only: false,
            }))
            .expect("custom-tick order should build");

        assert_eq!(order.price, Some(0.3815));
    }

    #[test]
    fn validate_unsigned_order_rejects_zero_price() {
        let mut order = test_unsigned_order_for_signer();
        order.price = Some(0.0);

        assert!(validate_unsigned_order(&order).is_err());
    }

    #[test]
    fn generate_salt_is_monotonic() {
        let builder = OrderBuilder::new(TEST_SIGNER_ADDRESS, 300, None);
        let first = builder.generate_salt();
        let second = builder.generate_salt();

        assert!(second > first);
    }

    #[test]
    fn div_ceil_handles_negative_operands() {
        assert_eq!(
            div_ceil(&BigInt::from(-3), &BigInt::from(2)).unwrap(),
            BigInt::from(-1)
        );
        assert_eq!(
            div_ceil(&BigInt::from(-3), &BigInt::from(-2)).unwrap(),
            BigInt::from(2)
        );
        assert_eq!(
            div_ceil(&BigInt::from(3), &BigInt::from(-2)).unwrap(),
            BigInt::from(-1)
        );
    }

    #[test]
    fn validate_signed_order_rejects_bad_signature() {
        let order = SignedOrder {
            salt: 1,
            maker: TEST_SIGNER_ADDRESS.to_string(),
            signer: TEST_SIGNER_ADDRESS.to_string(),
            taker: ZERO_ADDRESS.to_string(),
            token_id: "12345".to_string(),
            maker_amount: 1,
            taker_amount: 1,
            expiration: "0".to_string(),
            nonce: 0,
            fee_rate_bps: 300,
            side: Side::Buy,
            signature_type: SignatureType::Eoa,
            price: Some(0.1),
            signature: "0x1234".to_string(),
        };

        let error = validate_signed_order(&order).expect_err("signature should be rejected");
        assert!(error.to_string().contains("signature"));
    }

    #[test]
    fn order_signer_derives_expected_address() {
        let signer = OrderSigner::new(TEST_PRIVATE_KEY_HEX).expect("private key should be valid");
        assert_eq!(signer.address(), TEST_SIGNER_ADDRESS);
    }

    #[test]
    fn order_signer_matches_go_signature_vector() {
        let signer = OrderSigner::new(TEST_PRIVATE_KEY_HEX).expect("private key should be valid");
        let signature = signer
            .sign_order(
                &test_unsigned_order_for_signer(),
                &OrderSigningConfig {
                    chain_id: 8453,
                    contract_address: "0xa4409D988CA2218d956BeEFD3874100F444f0DC3".to_string(),
                },
            )
            .expect("signing should succeed");

        assert_eq!(signature, EXPECTED_ORDER_SIGNATURE);
    }
}
