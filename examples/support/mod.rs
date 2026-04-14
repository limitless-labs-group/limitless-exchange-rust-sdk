#![allow(dead_code)]

use std::{env, io, sync::Arc};

use limitless_exchange_rust_sdk::{Client, ConsoleLogger, HmacCredentials, LogLevel, SharedLogger};

pub fn require_env(key: &str) -> String {
    let value = env::var(key).unwrap_or_default().trim().to_string();
    if value.is_empty() {
        panic!("{key} environment variable is required");
    }
    value
}

pub fn optional_env(key: &str) -> Option<String> {
    let value = env::var(key).unwrap_or_default().trim().to_string();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

pub fn optional_env_with_fallback(key: &str, fallback: &str) -> String {
    optional_env(key).unwrap_or_else(|| fallback.to_string())
}

pub fn env_flag(key: &str, fallback: bool) -> bool {
    match optional_env(key)
        .unwrap_or_else(|| if fallback { "true" } else { "false" }.to_string())
        .to_lowercase()
        .as_str()
    {
        "1" | "true" | "yes" | "on" => true,
        "0" | "false" | "no" | "off" => false,
        _ => panic!("{key} must be one of 1,true,yes,on,0,false,no,off"),
    }
}

pub fn optional_positive_i32(key: &str) -> Option<i32> {
    optional_env(key).map(|value| {
        value
            .parse::<i32>()
            .ok()
            .filter(|parsed| *parsed > 0)
            .unwrap_or_else(|| panic!("{key} must be a positive integer"))
    })
}

pub fn optional_non_negative_u64(key: &str, fallback: u64) -> u64 {
    optional_env(key)
        .map(|value| {
            value
                .parse::<u64>()
                .unwrap_or_else(|_| panic!("{key} must be a zero-or-positive integer"))
        })
        .unwrap_or(fallback)
}

pub fn logger() -> SharedLogger {
    Arc::new(ConsoleLogger::new(LogLevel::Info))
}

pub fn public_client() -> Result<Client, Box<dyn std::error::Error>> {
    Ok(Client::from_http_client(
        Client::builder().logger(logger()).build()?,
    )?)
}

pub fn api_key_client() -> Result<Client, Box<dyn std::error::Error>> {
    Ok(Client::from_http_client(
        Client::builder()
            .api_key(require_env("LIMITLESS_API_KEY"))
            .logger(logger())
            .build()?,
    )?)
}

pub fn hmac_client() -> Result<Client, Box<dyn std::error::Error>> {
    Ok(Client::from_http_client(
        Client::builder()
            .hmac_credentials(HmacCredentials {
                token_id: require_env("LIMITLESS_API_TOKEN_ID"),
                secret: require_env("LIMITLESS_API_TOKEN_SECRET"),
            })
            .logger(logger())
            .build()?,
    )?)
}

pub fn hmac_or_api_key_client() -> Result<Client, Box<dyn std::error::Error>> {
    if optional_env("LIMITLESS_API_TOKEN_ID").is_some()
        || optional_env("LIMITLESS_API_TOKEN_SECRET").is_some()
    {
        if optional_env("LIMITLESS_API_TOKEN_ID").is_none()
            || optional_env("LIMITLESS_API_TOKEN_SECRET").is_none()
        {
            return Err(io::Error::other(
                "both LIMITLESS_API_TOKEN_ID and LIMITLESS_API_TOKEN_SECRET are required for scoped API-key auth",
            )
            .into());
        }
        hmac_client()
    } else {
        api_key_client()
    }
}

pub fn required_market_tokens(
    market: &limitless_exchange_rust_sdk::Market,
) -> Result<limitless_exchange_rust_sdk::MarketTokens, Box<dyn std::error::Error>> {
    market
        .tokens
        .clone()
        .ok_or_else(|| io::Error::other("market has no tokens").into())
}

pub fn wait_for_ctrl_c() -> Result<(), Box<dyn std::error::Error>> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        tokio::signal::ctrl_c().await?;
        Ok::<(), Box<dyn std::error::Error>>(())
    })
}

pub fn empty_fallback(value: Option<&str>, fallback: &str) -> String {
    value
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback)
        .to_string()
}
