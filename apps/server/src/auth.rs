use axum::{
    Json,
    extract::{Request, State},
    http::{HeaderValue, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::Serialize;
use subtle::ConstantTimeEq;

use crate::{config::BasicAuth, request_id::RequestCorrelationId};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AuthenticationError {
    code: &'static str,
    message: &'static str,
    correlation_id: String,
    retryable: bool,
}

pub async fn require_basic_auth(
    State(auth): State<BasicAuth>,
    request: Request,
    next: Next,
) -> Response {
    let authorized = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| credentials_match(value, &auth));

    if authorized {
        return next.run(request).await;
    }

    let correlation_id = request
        .extensions()
        .get::<RequestCorrelationId>()
        .map_or_else(|| "unknown".to_owned(), |value| value.0.clone());
    let mut response = (
        StatusCode::UNAUTHORIZED,
        Json(AuthenticationError {
            code: "unauthorized",
            message: "Authentication is required to access MKV Orchestrator.",
            correlation_id,
            retryable: false,
        }),
    )
        .into_response();
    response.headers_mut().insert(
        header::WWW_AUTHENTICATE,
        HeaderValue::from_static("Basic realm=\"MKV Orchestrator\", charset=\"UTF-8\""),
    );
    response
}

fn credentials_match(header_value: &str, auth: &BasicAuth) -> bool {
    let Some((scheme, encoded)) = header_value.split_once(' ') else {
        return false;
    };
    if !scheme.eq_ignore_ascii_case("basic") {
        return false;
    }
    let Ok(decoded) = STANDARD.decode(encoded) else {
        return false;
    };
    let expected = format!("{}:{}", auth.username, auth.password);
    decoded.len() == expected.len() && bool::from(decoded.ct_eq(expected.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_basic_credentials() {
        let auth = BasicAuth {
            username: "mkvo".to_owned(),
            password: "secret".to_owned(),
        };
        assert!(credentials_match("Basic bWt2bzpzZWNyZXQ=", &auth));
        assert!(!credentials_match("Basic bWt2bzpiYWQ=", &auth));
        assert!(!credentials_match("Bearer token", &auth));
    }

    #[test]
    fn rejects_malformed_base64() {
        let auth = BasicAuth {
            username: "mkvo".to_owned(),
            password: "secret".to_owned(),
        };
        assert!(!credentials_match("Basic not-base64!", &auth));
    }
}
