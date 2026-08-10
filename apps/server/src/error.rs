use axum::{
    Json,
    http::{HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use mkvo_contracts::ApiErrorCode;
use mkvo_runtime::RuntimeError;
use tracing::{error, warn};

use crate::request_id::{CORRELATION_ID_HEADER, REQUEST_CORRELATION_ID};

#[derive(Debug)]
pub struct HttpError(pub RuntimeError);

impl From<RuntimeError> for HttpError {
    fn from(value: RuntimeError) -> Self {
        Self(value)
    }
}

impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        let mut error = self.0;
        if let Ok(correlation_id) = REQUEST_CORRELATION_ID.try_with(Clone::clone)
            && let Ok(correlation_id) = correlation_id.parse()
        {
            error.correlation_id = correlation_id;
        }
        let status = status_for_code(error.code);
        if status.is_server_error() {
            error!(
                correlation_id = %error.correlation_id,
                code = ?error.code,
                retryable = error.retryable,
                message = %error.message,
                "runtime request failed"
            );
        } else {
            warn!(
                correlation_id = %error.correlation_id,
                code = ?error.code,
                message = %error.message,
                "runtime request rejected"
            );
        }

        let correlation_id = HeaderValue::from_str(&error.correlation_id.to_string())
            .expect("a runtime correlation UUID is always a valid HTTP header value");
        let mut response = (status, Json(error)).into_response();
        response
            .headers_mut()
            .insert(CORRELATION_ID_HEADER.clone(), correlation_id);
        response
    }
}

const fn status_for_code(code: ApiErrorCode) -> StatusCode {
    match code {
        ApiErrorCode::InvalidRequest => StatusCode::BAD_REQUEST,
        ApiErrorCode::UnauthorizedPath => StatusCode::FORBIDDEN,
        ApiErrorCode::NotFound => StatusCode::NOT_FOUND,
        ApiErrorCode::Conflict
        | ApiErrorCode::PlanStale
        | ApiErrorCode::PlanTampered
        | ApiErrorCode::JobCanceled => StatusCode::CONFLICT,
        ApiErrorCode::PlanExpired => StatusCode::GONE,
        ApiErrorCode::ToolUnavailable
        | ApiErrorCode::ProviderUnavailable
        | ApiErrorCode::CacheUnavailable => StatusCode::SERVICE_UNAVAILABLE,
        ApiErrorCode::RateLimited => StatusCode::TOO_MANY_REQUESTS,
        ApiErrorCode::JobFailed | ApiErrorCode::Internal => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_runtime_codes_to_http_statuses() {
        assert_eq!(
            status_for_code(ApiErrorCode::InvalidRequest),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            status_for_code(ApiErrorCode::UnauthorizedPath),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            status_for_code(ApiErrorCode::NotFound),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            status_for_code(ApiErrorCode::Conflict),
            StatusCode::CONFLICT
        );
        assert_eq!(status_for_code(ApiErrorCode::PlanExpired), StatusCode::GONE);
        assert_eq!(
            status_for_code(ApiErrorCode::ToolUnavailable),
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(
            status_for_code(ApiErrorCode::RateLimited),
            StatusCode::TOO_MANY_REQUESTS
        );
        assert_eq!(
            status_for_code(ApiErrorCode::Internal),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }
}
