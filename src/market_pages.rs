use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::{form_urlencoded::Serializer, Url};

use crate::{
    errors::{LimitlessError, Result},
    http_client::{HttpClient, RequestOptions},
    markets::Market,
};

const MAX_REDIRECT_DEPTH: usize = 3;

#[derive(Clone)]
pub struct MarketPageFetcher {
    client: HttpClient,
}

impl MarketPageFetcher {
    pub fn new(client: HttpClient) -> Self {
        Self { client }
    }

    pub async fn get_navigation(&self) -> Result<Vec<NavigationNode>> {
        self.client.get("/navigation").await
    }

    pub async fn get_market_page_by_path(&self, path: &str) -> Result<MarketPage> {
        let mut current_path = path.to_string();

        for depth in 0..=MAX_REDIRECT_DEPTH {
            let mut query = Serializer::new(String::new());
            query.append_pair("path", &current_path);
            let endpoint = format!("/market-pages/by-path?{}", query.finish());

            let response = self
                .client
                .get_raw(&endpoint, RequestOptions::default().allow_status(301))
                .await?;

            if response.status == 200 {
                return response.json();
            }
            if response.status != 301 {
                return Err(LimitlessError::invalid_input(format!(
                    "unexpected response status: {}",
                    response.status
                )));
            }
            if depth >= MAX_REDIRECT_DEPTH {
                return Err(LimitlessError::invalid_input(format!(
                    "too many redirects while resolving market page path '{path}' (max {MAX_REDIRECT_DEPTH})"
                )));
            }

            let location = response
                .headers
                .get("location")
                .and_then(|value| value.to_str().ok())
                .ok_or_else(|| {
                    LimitlessError::invalid_input("redirect response missing valid Location header")
                })?;

            current_path = self.extract_redirect_path(location)?;
        }

        Err(LimitlessError::invalid_input(
            "failed to resolve market page path after redirects",
        ))
    }

    pub(crate) fn extract_redirect_path(&self, location: &str) -> Result<String> {
        const DIRECT_BY_PATH_PREFIX: &str = "/market-pages/by-path";

        if location.starts_with(DIRECT_BY_PATH_PREFIX) {
            let parsed = Url::parse(&format!("https://api.limitless.exchange{location}"))
                .map_err(|err| LimitlessError::invalid_input(err.to_string()))?;
            return parsed
                .query_pairs()
                .find(|(key, _)| key == "path")
                .map(|(_, value)| value.to_string())
                .ok_or_else(|| {
                    LimitlessError::invalid_input(format!(
                        "redirect location '{location}' is missing required 'path' query parameter"
                    ))
                });
        }

        if location.starts_with("http://") || location.starts_with("https://") {
            let parsed = Url::parse(location)
                .map_err(|err| LimitlessError::invalid_input(err.to_string()))?;
            if parsed.path() == DIRECT_BY_PATH_PREFIX {
                return parsed
                    .query_pairs()
                    .find(|(key, _)| key == "path")
                    .map(|(_, value)| value.to_string())
                    .ok_or_else(|| {
                        LimitlessError::invalid_input(format!(
                            "redirect location '{location}' is missing required 'path' query parameter"
                        ))
                    });
            }
            return Ok(parsed.path().to_string());
        }

        Ok(location.to_string())
    }

    pub async fn get_markets(
        &self,
        page_id: &str,
        params: Option<&MarketPageMarketsParams>,
    ) -> Result<MarketPageMarketsResponse> {
        if let Some(params) = params {
            if params.cursor.is_some() && params.page.is_some() {
                return Err(LimitlessError::invalid_input(
                    "parameters `cursor` and `page` are mutually exclusive",
                ));
            }
        }

        let mut query = Serializer::new(String::new());
        if let Some(params) = params {
            if let Some(page) = params.page {
                query.append_pair("page", &page.to_string());
            }
            if let Some(limit) = params.limit {
                query.append_pair("limit", &limit.to_string());
            }
            if let Some(sort) = &params.sort {
                query.append_pair("sort", sort.as_ref());
            }
            if let Some(cursor) = &params.cursor {
                query.append_pair("cursor", cursor);
            }
            for (key, value) in &params.filters {
                if let Some(items) = value.as_array() {
                    for item in items {
                        query.append_pair(key, &stringify_filter_value(item));
                    }
                } else {
                    query.append_pair(key, &stringify_filter_value(value));
                }
            }
        }

        let encoded = query.finish();
        let mut endpoint = format!("/market-pages/{}/markets", urlencoding::encode(page_id));
        if !encoded.is_empty() {
            endpoint.push('?');
            endpoint.push_str(&encoded);
        }

        let response: MarketPageMarketsResponse = self.client.get(&endpoint).await?;
        if response.pagination.is_none() && response.cursor.is_none() {
            return Err(LimitlessError::invalid_input(
                "invalid market-page response: expected `pagination` or `cursor` metadata",
            ));
        }
        Ok(response)
    }

    pub async fn get_property_keys(&self) -> Result<Vec<PropertyKey>> {
        self.client.get("/property-keys").await
    }

    pub async fn get_property_key(&self, id: &str) -> Result<PropertyKey> {
        self.client
            .get(&format!("/property-keys/{}", urlencoding::encode(id)))
            .await
    }

    pub async fn get_property_options(
        &self,
        key_id: &str,
        parent_id: Option<&str>,
    ) -> Result<Vec<PropertyOption>> {
        let mut endpoint = format!("/property-keys/{}/options", urlencoding::encode(key_id));
        if let Some(parent_id) = parent_id.filter(|value| !value.is_empty()) {
            let mut query = Serializer::new(String::new());
            query.append_pair("parentId", parent_id);
            endpoint.push('?');
            endpoint.push_str(&query.finish());
        }
        self.client.get(&endpoint).await
    }
}

fn stringify_filter_value(value: &Value) -> String {
    match value {
        Value::Bool(flag) => flag.to_string(),
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        Value::Null => String::new(),
        _ => value.to_string(),
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NavigationNode {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub path: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub children: Vec<NavigationNode>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FilterGroupOption {
    pub label: String,
    pub value: String,
    #[serde(default)]
    pub metadata: Option<HashMap<String, Value>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FilterGroup {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub slug: Option<String>,
    #[serde(rename = "allowMultiple", default)]
    pub allow_multiple: Option<bool>,
    #[serde(default)]
    pub presentation: Option<String>,
    #[serde(default)]
    pub options: Vec<FilterGroupOption>,
    #[serde(default)]
    pub source: Option<HashMap<String, Value>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BreadcrumbItem {
    pub name: String,
    pub slug: String,
    pub path: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MarketPage {
    pub id: String,
    pub name: String,
    pub slug: String,
    #[serde(rename = "fullPath")]
    pub full_path: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(rename = "baseFilter")]
    pub base_filter: HashMap<String, Value>,
    #[serde(rename = "filterGroups")]
    pub filter_groups: Vec<FilterGroup>,
    pub metadata: HashMap<String, Value>,
    pub breadcrumb: Vec<BreadcrumbItem>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PropertyOption {
    pub id: String,
    #[serde(rename = "propertyKeyId")]
    pub property_key_id: String,
    pub value: String,
    pub label: String,
    #[serde(rename = "sortOrder")]
    pub sort_order: i32,
    #[serde(rename = "parentOptionId", default)]
    pub parent_option_id: Option<String>,
    pub metadata: HashMap<String, Value>,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PropertyKey {
    pub id: String,
    pub name: String,
    pub slug: String,
    #[serde(rename = "type")]
    pub property_type: String,
    pub metadata: HashMap<String, Value>,
    #[serde(rename = "isSystem")]
    pub is_system: bool,
    #[serde(default)]
    pub options: Vec<PropertyOption>,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OffsetPagination {
    pub page: i32,
    pub limit: i32,
    pub total: i32,
    #[serde(rename = "totalPages")]
    pub total_pages: i32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CursorPagination {
    #[serde(rename = "nextCursor", default)]
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum MarketPageSort {
    #[serde(rename = "createdAt")]
    CreatedAt,
    #[serde(rename = "-createdAt")]
    CreatedAtDesc,
    #[serde(rename = "updatedAt")]
    UpdatedAt,
    #[serde(rename = "-updatedAt")]
    UpdatedAtDesc,
    #[serde(rename = "deadline")]
    Deadline,
    #[serde(rename = "-deadline")]
    DeadlineDesc,
    #[serde(rename = "id")]
    Id,
    #[serde(rename = "-id")]
    IdDesc,
}

impl AsRef<str> for MarketPageSort {
    fn as_ref(&self) -> &str {
        match self {
            Self::CreatedAt => "createdAt",
            Self::CreatedAtDesc => "-createdAt",
            Self::UpdatedAt => "updatedAt",
            Self::UpdatedAtDesc => "-updatedAt",
            Self::Deadline => "deadline",
            Self::DeadlineDesc => "-deadline",
            Self::Id => "id",
            Self::IdDesc => "-id",
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MarketPageMarketsParams {
    pub page: Option<u32>,
    pub limit: Option<u32>,
    pub sort: Option<MarketPageSort>,
    pub cursor: Option<String>,
    #[serde(default)]
    pub filters: HashMap<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MarketPageMarketsResponse {
    pub data: Vec<Market>,
    #[serde(default)]
    pub pagination: Option<OffsetPagination>,
    #[serde(default)]
    pub cursor: Option<CursorPagination>,
}

#[cfg(test)]
mod tests {
    use super::MarketPageFetcher;
    use crate::http_client::HttpClient;

    #[test]
    fn extracts_direct_redirect_query_path() {
        let client = HttpClient::builder().build().unwrap();
        let fetcher = MarketPageFetcher::new(client);
        let value = fetcher
            .extract_redirect_path("/market-pages/by-path?path=%2Fsports%2Fnba")
            .unwrap();
        assert_eq!(value, "/sports/nba");
    }
}
