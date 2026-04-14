use crate::{
    errors::{LimitlessError, Result},
    http_client::HttpClient,
    logger::{noop_logger, SharedLogger},
    orders::{
        post_only_from_args, CancelResponse, OrderArgs, OrderBuilder, OrderResponse, OrderType,
        Side, SignatureType,
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
        self.client.require_auth("CreateDelegatedOrder")?;
        if params.on_behalf_of <= 0 {
            return Err(LimitlessError::invalid_input(
                "OnBehalfOf must be a positive integer",
            ));
        }

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
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
