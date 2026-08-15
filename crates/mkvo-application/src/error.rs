use std::path::PathBuf;

use mkvo_contracts::{ApiError, ApiErrorCode};
use mkvo_domain::{CorrelationId, PlanValidationError};

pub type ApplicationResult<T> = Result<T, ApplicationError>;

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum PortError {
    #[error("{message}")]
    Unavailable { message: String, retryable: bool },
    #[error("not found: {0}")]
    NotFound(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("invalid data from adapter: {0}")]
    InvalidData(String),
    #[error("operation was canceled")]
    Canceled,
    #[error("adapter failure: {0}")]
    Other(String),
}

impl PortError {
    #[must_use]
    pub fn unavailable(message: impl Into<String>, retryable: bool) -> Self {
        Self::Unavailable {
            message: message.into(),
            retryable,
        }
    }

    #[must_use]
    pub const fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::Unavailable {
                retryable: true,
                ..
            }
        )
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ApplicationError {
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    #[error("path is outside an authorized root: {0}")]
    UnauthorizedPath(PathBuf),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("operation was canceled")]
    Canceled,
    #[error("plan validation failed: {0}")]
    Plan(#[from] PlanValidationError),
    #[error("{message}")]
    DependencyUnavailable { message: String, retryable: bool },
    #[error("adapter failure: {0}")]
    AdapterFailure(String),
    #[error("internal error: {0}")]
    Internal(String),
}

impl ApplicationError {
    #[must_use]
    pub fn into_api_error(self, correlation_id: CorrelationId) -> ApiError {
        let retryable = matches!(
            &self,
            Self::DependencyUnavailable {
                retryable: true,
                ..
            }
        );
        let code = match &self {
            Self::InvalidRequest(_) => ApiErrorCode::InvalidRequest,
            Self::UnauthorizedPath(_) => ApiErrorCode::UnauthorizedPath,
            Self::NotFound(_) => ApiErrorCode::NotFound,
            Self::Conflict(_) => ApiErrorCode::Conflict,
            Self::Canceled => ApiErrorCode::JobCanceled,
            Self::Plan(PlanValidationError::Expired(_)) => ApiErrorCode::PlanExpired,
            Self::Plan(PlanValidationError::FingerprintMismatch) => ApiErrorCode::PlanTampered,
            Self::Plan(_) => ApiErrorCode::PlanStale,
            // A port failure is usually something the user can act on — an
            // unconfigured provider, an unreachable server, a stale reference.
            // Collapsing them all to `Internal` tells the user to report a bug
            // when they should be opening Settings.
            Self::DependencyUnavailable { .. } => ApiErrorCode::ProviderUnavailable,
            Self::AdapterFailure(_) | Self::Internal(_) => ApiErrorCode::Internal,
        };
        ApiError::new(code, self.to_string(), correlation_id).retryable(retryable)
    }
}

impl From<PortError> for ApplicationError {
    fn from(error: PortError) -> Self {
        match error {
            PortError::Unavailable { message, retryable } => {
                Self::DependencyUnavailable { message, retryable }
            }
            PortError::NotFound(message) => Self::NotFound(message),
            PortError::Conflict(message) => Self::Conflict(message),
            PortError::InvalidData(message) => Self::InvalidRequest(message),
            PortError::Canceled => Self::Canceled,
            PortError::Other(message) => Self::AdapterFailure(message),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Port failures are mostly user-actionable: an unconfigured provider, an
    /// unreachable server, a stale reference. Reporting them as `Internal` sends
    /// the user to file a bug instead of to Settings, and makes every adapter
    /// fault a 500 rather than the status that describes it.
    #[test]
    fn port_failures_keep_their_actionable_error_code() {
        let cases = [
            (
                PortError::unavailable("AniDB credentials are not configured", false),
                ApiErrorCode::ProviderUnavailable,
            ),
            (
                PortError::NotFound("plan".to_owned()),
                ApiErrorCode::NotFound,
            ),
            (
                PortError::Conflict("idempotency".to_owned()),
                ApiErrorCode::Conflict,
            ),
            (
                PortError::InvalidData("bad payload".to_owned()),
                ApiErrorCode::InvalidRequest,
            ),
            (
                PortError::Other("disk exploded".to_owned()),
                ApiErrorCode::Internal,
            ),
        ];

        for (port, expected) in cases {
            let api = ApplicationError::from(port.clone())
                .into_api_error(mkvo_domain::CorrelationId::new());
            assert_eq!(api.code, expected, "{port:?}");
        }
    }

    #[test]
    fn a_retryable_port_failure_stays_marked_retryable() {
        let api = ApplicationError::from(PortError::unavailable("provider timed out", true))
            .into_api_error(mkvo_domain::CorrelationId::new());
        assert!(api.retryable);
    }
}
