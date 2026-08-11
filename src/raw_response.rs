use crate::http_client::RawResponse;

/// Pairs a decoded, typed value with the raw HTTP response it was parsed from.
///
/// Every API-backed service method has a sibling `*_with_raw` variant returning
/// `Result<SdkResponse<T>>`. Use it when you need status codes, response headers,
/// or the exact response bytes alongside the decoded value. The base method
/// (without `_with_raw`) returns just `T` and is unchanged.
#[derive(Clone, Debug)]
pub struct SdkResponse<T> {
    /// The decoded, typed response value (identical to what the base method returns).
    pub data: T,
    /// The underlying raw HTTP response: status code, headers, and body bytes.
    pub raw: RawResponse,
}

impl<T> SdkResponse<T> {
    /// Constructs a wrapper from a decoded value and its raw response.
    pub fn new(data: T, raw: RawResponse) -> Self {
        Self { data, raw }
    }

    /// Consumes the wrapper and returns the decoded value, discarding the raw response.
    pub fn into_data(self) -> T {
        self.data
    }

    /// Maps the decoded value while preserving the raw response.
    pub fn map<U, F: FnOnce(T) -> U>(self, f: F) -> SdkResponse<U> {
        SdkResponse {
            data: f(self.data),
            raw: self.raw,
        }
    }
}

#[cfg(test)]
mod tests {
    use reqwest::header::{HeaderMap, HeaderValue};
    use serde::Deserialize;

    use super::SdkResponse;
    use crate::http_client::RawResponse;

    #[derive(Debug, Deserialize, PartialEq)]
    struct Sample {
        id: i32,
        name: String,
    }

    fn synthetic_raw(status: u16, body: &str) -> RawResponse {
        let mut headers = HeaderMap::new();
        headers.insert("content-type", HeaderValue::from_static("application/json"));
        RawResponse {
            status,
            headers,
            body: body.as_bytes().to_vec(),
        }
    }

    #[test]
    fn raw_response_json_decodes_typed_body() {
        let raw = synthetic_raw(200, r#"{"id":7,"name":"limitless"}"#);
        let value: Sample = raw.json().expect("body should decode");
        assert_eq!(
            value,
            Sample {
                id: 7,
                name: "limitless".to_string()
            }
        );
    }

    #[test]
    fn sdk_response_construction_exposes_data_and_raw() {
        let raw = synthetic_raw(201, r#"{"id":1,"name":"a"}"#);
        let data: Sample = raw.json().unwrap();
        let response = SdkResponse::new(data, raw);

        assert_eq!(response.data.id, 1);
        assert_eq!(response.raw.status, 201);
        assert_eq!(
            response
                .raw
                .headers
                .get("content-type")
                .and_then(|value| value.to_str().ok()),
            Some("application/json")
        );
    }

    #[test]
    fn sdk_response_map_preserves_raw() {
        let raw = synthetic_raw(200, r#"{"id":3,"name":"x"}"#);
        let response = SdkResponse::new(3_i32, raw);
        let mapped = response.map(|value| value * 2);
        assert_eq!(mapped.data, 6);
        assert_eq!(mapped.raw.status, 200);
    }
}
