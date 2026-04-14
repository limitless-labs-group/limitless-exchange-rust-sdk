use crate::errors::{LimitlessError, Result};

pub const DEFAULT_API_URL: &str = "https://api.limitless.exchange";
pub const DEFAULT_WS_URL: &str = "wss://ws.limitless.exchange";
pub const DEFAULT_CHAIN_ID: u64 = 8453;
pub const BASE_SEPOLIA_CHAIN_ID: u64 = 84532;
pub const ZERO_ADDRESS: &str = "0x0000000000000000000000000000000000000000";
pub const SIGNING_MESSAGE_TEMPLATE: &str =
    "Welcome to Limitless.exchange! Please sign this message to verify your identity.\n\nNonce: {NONCE}";

pub fn get_contract_address(contract_type: &str, chain_id: Option<u64>) -> Result<&'static str> {
    match (chain_id.unwrap_or(DEFAULT_CHAIN_ID), contract_type) {
        (8453, "USDC") => Ok("0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"),
        (8453, "CTF") => Ok("0xC9c98965297Bc527861c898329Ee280632B76e18"),
        (84532, "USDC") | (84532, "CTF") => Err(LimitlessError::invalid_input(format!(
            "contract address for {contract_type} not available on chainId 84532"
        ))),
        (cid, _) => Err(LimitlessError::invalid_input(format!(
            "no contract addresses configured for chainId {cid}"
        ))),
    }
}
