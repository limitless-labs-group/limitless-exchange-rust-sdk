use crate::LimitlessError;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PackErrorKind {
    #[default]
    Other,
    AllowanceRequired,
    NotSubmitted,
    PriceDrifted,
    IntentExpired,
    WalletBusy,
    RateLimited,
    SubmissionUnknown,
    TransactionReverted,
}

/// Recovery metadata only; classification never retries or changes a purchase key.
#[derive(Clone, Debug, Default)]
pub struct PackErrorDetails {
    pub kind: PackErrorKind,
    pub code: Option<String>,
    pub message: Option<String>,
    pub submission_state: Option<String>,
    pub parlay_id: Option<String>,
    pub recovery: Option<String>,
    pub token_address: Option<String>,
    pub spender: Option<String>,
    pub required_allowance: Option<String>,
}

pub fn classify_pack_error(error: &LimitlessError) -> PackErrorDetails {
    let LimitlessError::Api(error) = error else {
        return PackErrorDetails::default();
    };
    let string = |key| {
        error
            .data
            .get(key)
            .and_then(|v| v.as_str())
            .map(str::to_owned)
    };
    let mut details = PackErrorDetails {
        code: string("code"),
        message: Some(error.message.clone()),
        submission_state: string("submissionState"),
        parlay_id: string("parlayId"),
        recovery: string("recovery"),
        token_address: string("tokenAddress"),
        spender: string("spender"),
        required_allowance: string("requiredAllowance"),
        ..Default::default()
    };
    details.kind = match (error.status, details.code.as_deref()) {
        (409, Some("PACK_SUBMISSION_UNKNOWN")) => PackErrorKind::SubmissionUnknown,
        (400..=499, _) if error.status != 429
            && details.submission_state.as_deref() == Some("NOT_SUBMITTED") =>
        {
            PackErrorKind::NotSubmitted
        }
        (409, Some("PACK_ALLOWANCE_REQUIRED")) => PackErrorKind::AllowanceRequired,
        (409, Some("PRICE_DRIFTED")) => PackErrorKind::PriceDrifted,
        (401, _) if error.message == "Intent expired — please retry" => PackErrorKind::IntentExpired,
        (409, _) if error.message == "Another Pack operation is in progress for this wallet; retry the same request" => PackErrorKind::WalletBusy,
        (429, _) => PackErrorKind::RateLimited,
        (409, Some("PACK_TRANSACTION_REVERTED")) => PackErrorKind::TransactionReverted,
        _ => PackErrorKind::Other,
    };
    details
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn packs_error_metadata_and_conservative_classification() {
        for (status, data, kind) in [
            (
                409,
                json!({"code":"PACK_ALLOWANCE_REQUIRED","tokenAddress":"0xtoken","spender":"0xvault","requiredAllowance":"1000000"}),
                PackErrorKind::AllowanceRequired,
            ),
            (
                400,
                json!({"code":"BAD_INTENT_SIGNATURE","submissionState":"NOT_SUBMITTED","parlayId":"position","recovery":"Fix cause"}),
                PackErrorKind::NotSubmitted,
            ),
            (
                409,
                json!({"code":"PRICE_DRIFTED"}),
                PackErrorKind::PriceDrifted,
            ),
            (
                401,
                json!({"message":"Intent expired — please retry"}),
                PackErrorKind::IntentExpired,
            ),
            (
                401,
                json!({"message":"Invalid signature"}),
                PackErrorKind::Other,
            ),
            (
                409,
                json!({"message":"Another Pack operation is in progress for this wallet; retry the same request"}),
                PackErrorKind::WalletBusy,
            ),
            (429, json!({}), PackErrorKind::RateLimited),
            (
                409,
                json!({"code":"PACK_TRANSACTION_REVERTED"}),
                PackErrorKind::TransactionReverted,
            ),
            (
                409,
                json!({"code":"PACK_SUBMISSION_UNKNOWN","submissionState":"NOT_SUBMITTED"}),
                PackErrorKind::SubmissionUnknown,
            ),
            (
                500,
                json!({"submissionState":"NOT_SUBMITTED"}),
                PackErrorKind::Other,
            ),
            (400, json!({"submissionState":true}), PackErrorKind::Other),
        ] {
            let err = crate::errors::parse_api_error(
                reqwest::StatusCode::from_u16(status).unwrap(),
                data.to_string().as_bytes(),
                "/parlay/partner/buy",
                "POST",
            );
            let details = classify_pack_error(&err);
            assert_eq!(details.kind, kind);
            if kind == PackErrorKind::AllowanceRequired {
                assert_eq!(details.token_address.as_deref(), Some("0xtoken"));
                assert_eq!(details.spender.as_deref(), Some("0xvault"));
                assert_eq!(details.required_allowance.as_deref(), Some("1000000"));
            }
            if kind == PackErrorKind::NotSubmitted {
                assert_eq!(details.parlay_id.as_deref(), Some("position"));
                assert_eq!(details.recovery.as_deref(), Some("Fix cause"));
            }
        }
        assert_eq!(
            classify_pack_error(&LimitlessError::invalid_input("unknown")).kind,
            PackErrorKind::Other
        );
    }
}
