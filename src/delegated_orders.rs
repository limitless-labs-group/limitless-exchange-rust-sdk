use crate::{
    errors::{LimitlessError, Result},
    http_client::HttpClient,
    logger::{noop_logger, SharedLogger},
    orders::{
        normalize_receive_window_options, post_only_from_args, CancelResponse, OrderArgs,
        OrderBuilder, OrderResponse, OrderType, ReceiveWindowOptions, Side, SignatureType,
    },
};
use serde::{Deserialize, Serialize};

const DEFAULT_DELEGATED_FEE_RATE_BPS: i32 = 300;

#[derive(Clone)]
pub struct DelegatedOrderService {
    client: HttpClient,
    logger: SharedLogger,
}

impl DelegatedOrderService {
    pub fn new(client: HttpClient, logger: Option<SharedLogger>) -> Self {
        Self {
            client,
            logger: logger.unwrap_or_else(noop_logger),
        }
    }

    pub async fn create_order(&self, params: CreateDelegatedOrderParams) -> Result<OrderResponse> {
        self.create_order_internal(params, None).await
    }

    /// Creates a delegated order with optional receive-window freshness controls.
    ///
    /// `timestamp` and `recv_window` are serialized as top-level `POST /orders`
    /// fields only. They are not part of the delegated order body.
    pub async fn create_order_with_receive_window(
        &self,
        params: CreateDelegatedOrderParams,
        receive_window: ReceiveWindowOptions,
    ) -> Result<OrderResponse> {
        self.create_order_internal(params, Some(receive_window))
            .await
    }

    async fn create_order_internal(
        &self,
        params: CreateDelegatedOrderParams,
        receive_window: Option<ReceiveWindowOptions>,
    ) -> Result<OrderResponse> {
        self.client.require_auth("CreateDelegatedOrder")?;
        if params.on_behalf_of <= 0 {
            return Err(LimitlessError::invalid_input(
                "OnBehalfOf must be a positive integer",
            ));
        }
        let receive_window =
            normalize_receive_window_options(receive_window, crate::orders::current_unix_ms)?;

        let fee_rate_bps = if params.fee_rate_bps <= 0 {
            DEFAULT_DELEGATED_FEE_RATE_BPS
        } else {
            params.fee_rate_bps
        };

        let builder = OrderBuilder::new(crate::constants::ZERO_ADDRESS, fee_rate_bps, None);
        let unsigned_order = builder.build_order(&params.args)?;

        self.logger.info("Creating delegated order");

        let payload = CreateOrderRequest {
            order: OrderSubmission {
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
                signature_type: SignatureType::Eoa,
                price: unsigned_order.price,
                signature: None,
            },
            order_type: params.order_type,
            market_slug: params.market_slug,
            owner_id: params.on_behalf_of,
            on_behalf_of: Some(params.on_behalf_of),
            post_only: post_only_from_args(&params.args),
            timestamp: receive_window.timestamp,
            recv_window: receive_window.recv_window,
        };

        self.client.post("/orders", &payload).await
    }

    pub async fn cancel(&self, order_id: &str) -> Result<String> {
        self.client.require_auth("CancelDelegatedOrder")?;
        let response: CancelResponse = self
            .client
            .delete(&format!("/orders/{}", urlencoding::encode(order_id)))
            .await?;
        Ok(response.message)
    }

    pub async fn cancel_on_behalf_of(&self, order_id: &str, on_behalf_of: i32) -> Result<String> {
        self.client.require_auth("CancelDelegatedOrder")?;
        if on_behalf_of <= 0 {
            return Err(LimitlessError::invalid_input(
                "OnBehalfOf must be a positive integer",
            ));
        }

        let response: CancelResponse = self
            .client
            .delete(&format!(
                "/orders/{}?onBehalfOf={}",
                urlencoding::encode(order_id),
                on_behalf_of
            ))
            .await?;
        Ok(response.message)
    }

    pub async fn cancel_all(&self, market_slug: &str) -> Result<String> {
        self.client.require_auth("CancelAllDelegatedOrders")?;
        let response: CancelResponse = self
            .client
            .delete(&format!("/orders/all/{}", urlencoding::encode(market_slug)))
            .await?;
        Ok(response.message)
    }

    pub async fn cancel_all_on_behalf_of(
        &self,
        market_slug: &str,
        on_behalf_of: i32,
    ) -> Result<String> {
        self.client.require_auth("CancelAllDelegatedOrders")?;
        if on_behalf_of <= 0 {
            return Err(LimitlessError::invalid_input(
                "OnBehalfOf must be a positive integer",
            ));
        }

        let response: CancelResponse = self
            .client
            .delete(&format!(
                "/orders/all/{}?onBehalfOf={}",
                urlencoding::encode(market_slug),
                on_behalf_of
            ))
            .await?;
        Ok(response.message)
    }
}

#[derive(Clone, Debug)]
pub struct CreateDelegatedOrderParams {
    pub market_slug: String,
    pub order_type: OrderType,
    pub on_behalf_of: i32,
    pub fee_rate_bps: i32,
    pub args: OrderArgs,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrderSubmission {
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreateOrderRequest {
    pub order: OrderSubmission,
    #[serde(rename = "orderType")]
    pub order_type: OrderType,
    #[serde(rename = "marketSlug")]
    pub market_slug: String,
    #[serde(rename = "ownerId")]
    pub owner_id: i32,
    #[serde(rename = "onBehalfOf", skip_serializing_if = "Option::is_none")]
    pub on_behalf_of: Option<i32>,
    #[serde(rename = "postOnly", skip_serializing_if = "Option::is_none")]
    pub post_only: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<i64>,
    #[serde(rename = "recvWindow", skip_serializing_if = "Option::is_none")]
    pub recv_window: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn test_delegated_order_params() -> CreateDelegatedOrderParams {
        CreateDelegatedOrderParams {
            market_slug: "market".to_string(),
            order_type: OrderType::Gtc,
            on_behalf_of: 326,
            fee_rate_bps: 300,
            args: crate::orders::GtcOrderArgs {
                token_id: "123".to_string(),
                side: Side::Buy,
                price: 0.5,
                size: 1.0,
                expiration: None,
                nonce: None,
                taker: None,
                post_only: false,
            }
            .into(),
        }
    }

    fn test_order_submission() -> OrderSubmission {
        let params = test_delegated_order_params();
        let unsigned = OrderBuilder::new(crate::constants::ZERO_ADDRESS, 300, None)
            .build_order(&params.args)
            .expect("delegated order should build");

        OrderSubmission {
            salt: unsigned.salt,
            maker: unsigned.maker,
            signer: unsigned.signer,
            taker: unsigned.taker,
            token_id: unsigned.token_id,
            maker_amount: unsigned.maker_amount,
            taker_amount: unsigned.taker_amount,
            expiration: unsigned.expiration,
            nonce: unsigned.nonce,
            fee_rate_bps: unsigned.fee_rate_bps,
            side: unsigned.side,
            signature_type: SignatureType::Eoa,
            price: unsigned.price,
            signature: None,
        }
    }

    #[test]
    fn delegated_order_requires_positive_on_behalf_of() {
        let service = DelegatedOrderService::new(
            HttpClient::builder()
                .api_key("test-api-key")
                .build()
                .expect("client"),
            None,
        );
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        let error = runtime
            .block_on(
                service.create_order(CreateDelegatedOrderParams {
                    market_slug: "market".to_string(),
                    order_type: OrderType::Fok,
                    on_behalf_of: 0,
                    fee_rate_bps: 0,
                    args: crate::orders::FokOrderArgs {
                        token_id: "123".to_string(),
                        side: Side::Buy,
                        maker_amount: 1.0,
                        expiration: None,
                        nonce: None,
                        taker: None,
                    }
                    .into(),
                }),
            )
            .expect_err("on behalf of validation should fail");

        assert!(error.to_string().contains("OnBehalfOf"));
    }

    #[test]
    fn delegated_order_request_serializes_receive_window_top_level_only() {
        let request = CreateOrderRequest {
            order: test_order_submission(),
            order_type: OrderType::Gtc,
            market_slug: "market".to_string(),
            owner_id: 326,
            on_behalf_of: Some(326),
            post_only: None,
            timestamp: Some(1_770_000_000_000),
            recv_window: Some(1500),
        };

        let value = serde_json::to_value(&request).expect("request should serialize");
        assert_eq!(value["timestamp"], json!(1_770_000_000_000_i64));
        assert_eq!(value["recvWindow"], json!(1500));
        assert!(value["order"].get("timestamp").is_none());
        assert!(value["order"].get("recvWindow").is_none());
    }

    #[test]
    fn delegated_order_request_omits_receive_window_by_default() {
        let request = CreateOrderRequest {
            order: test_order_submission(),
            order_type: OrderType::Gtc,
            market_slug: "market".to_string(),
            owner_id: 326,
            on_behalf_of: Some(326),
            post_only: None,
            timestamp: None,
            recv_window: None,
        };

        let value = serde_json::to_value(&request).expect("request should serialize");
        assert!(value.get("timestamp").is_none());
        assert!(value.get("recvWindow").is_none());
        assert!(value["order"].get("timestamp").is_none());
        assert!(value["order"].get("recvWindow").is_none());
    }

    #[test]
    fn delegated_order_with_receive_window_rejects_invalid_values_before_network() {
        let service = DelegatedOrderService::new(
            HttpClient::builder()
                .api_key("test-api-key")
                .build()
                .expect("client"),
            None,
        );
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        let error = runtime
            .block_on(service.create_order_with_receive_window(
                test_delegated_order_params(),
                ReceiveWindowOptions {
                    timestamp: None,
                    recv_window: Some(0),
                },
            ))
            .expect_err("invalid receive-window options should fail");

        assert!(matches!(error, LimitlessError::InvalidInput(_)));
        assert!(error.to_string().contains("recv_window"));
    }
}
