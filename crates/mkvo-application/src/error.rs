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
    #[error(transparent)]
    Port(#[from] PortError),
    #[error("internal error: {0}")]
    Internal(String),
}

impl ApplicationError {
    #[must_use]
    pub fn into_api_error(self, correlation_id: CorrelationId) -> ApiError {
        let retryable = matches!(&self, Self::Port(error) if error.is_retryable());
        let code = match &self {
            Self::InvalidRequest(_) => ApiErrorCode::InvalidRequest,
            Self::UnauthorizedPath(_) => ApiErrorCode::UnauthorizedPath,
            Self::NotFound(_) => ApiErrorCode::NotFound,
            Self::Conflict(_) => ApiErrorCode::Conflict,
            Self::Canceled | Self::Port(PortError::Canceled) => ApiErrorCode::JobCanceled,
            Self::Plan(PlanValidationError::Expired(_)) => ApiErrorCode::PlanExpired,
            Self::Plan(PlanValidationError::FingerprintMismatch) => ApiErrorCode::PlanTampered,
            Self::Plan(_) => ApiErrorCode::PlanStale,
            Self::Port(_) | Self::Internal(_) => ApiErrorCode::Internal,
        };
        ApiError::new(code, self.to_string(), correlation_id).retryable(retryable)
    }
}
