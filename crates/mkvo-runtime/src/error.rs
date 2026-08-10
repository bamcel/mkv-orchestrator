use std::fmt;

use mkvo_application::{ApplicationError, PortError};
use mkvo_contracts::ApiErrorCode;
use mkvo_domain::CorrelationId;
use serde::{Deserialize, Serialize};

pub type RuntimeResult<T> = Result<T, RuntimeError>;

/// Stable, serializable error returned by both IPC and HTTP transports.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeError {
    pub code: ApiErrorCode,
    pub message: String,
    pub correlation_id: CorrelationId,
    #[serde(default)]
    pub retryable: bool,
}

impl RuntimeError {
    #[must_use]
    pub fn new(code: ApiErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            correlation_id: CorrelationId::new(),
            retryable: false,
        }
    }

    #[must_use]
    pub const fn retryable(mut self, retryable: bool) -> Self {
        self.retryable = retryable;
        self
    }

    #[must_use]
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ApiErrorCode::InvalidRequest, message)
    }

    #[must_use]
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(ApiErrorCode::NotFound, message)
    }

    #[must_use]
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ApiErrorCode::Internal, message)
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(formatter)
    }
}

impl std::error::Error for RuntimeError {}

impl From<ApplicationError> for RuntimeError {
    fn from(error: ApplicationError) -> Self {
        let api = error.into_api_error(CorrelationId::new());
        Self {
            code: api.code,
            message: api.message,
            correlation_id: api.correlation_id,
            retryable: api.retryable,
        }
    }
}

impl From<PortError> for RuntimeError {
    fn from(error: PortError) -> Self {
        ApplicationError::Port(error).into()
    }
}

impl From<serde_json::Error> for RuntimeError {
    fn from(error: serde_json::Error) -> Self {
        Self::internal(format!("contract serialization failed: {error}"))
    }
}

impl From<std::io::Error> for RuntimeError {
    fn from(error: std::io::Error) -> Self {
        Self::internal(format!("filesystem operation failed: {error}"))
    }
}
