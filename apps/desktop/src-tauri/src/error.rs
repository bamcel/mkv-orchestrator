use mkvo_runtime::{RuntimeError, RuntimeResult};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

pub type CommandResult = Result<Value, CommandError>;

/// Structured IPC error consumed by `normalizeApiError` in the React client.
#[derive(Debug, Serialize)]
#[serde(transparent)]
pub struct CommandError(RuntimeError);

impl From<RuntimeError> for CommandError {
    fn from(error: RuntimeError) -> Self {
        Self(error)
    }
}

pub fn decode_request<T: DeserializeOwned>(request: Value) -> Result<T, CommandError> {
    serde_json::from_value(request).map_err(|error| {
        CommandError::from(RuntimeError::invalid(format!(
            "invalid IPC request payload: {error}"
        )))
    })
}

pub fn encode_response<T: Serialize>(result: RuntimeResult<T>) -> CommandResult {
    let response = result.map_err(CommandError::from)?;
    serde_json::to_value(response).map_err(|error| {
        CommandError::from(RuntimeError::internal(format!(
            "could not serialize IPC response: {error}"
        )))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize, PartialEq)]
    #[serde(rename_all = "camelCase")]
    struct Request {
        source_path: String,
    }

    #[test]
    fn decodes_camel_case_frontend_requests() {
        let request = decode_request::<Request>(serde_json::json!({
            "sourcePath": "C:/Media"
        }))
        .expect("valid request");

        assert_eq!(request.source_path, "C:/Media");
    }

    #[test]
    fn invalid_requests_return_structured_errors() {
        let error = decode_request::<Request>(serde_json::json!({ "sourcePath": 42 }))
            .expect_err("invalid request");
        let serialized = serde_json::to_value(error).expect("serializable error");

        assert_eq!(serialized["code"], "invalid_request");
        assert!(
            serialized["message"]
                .as_str()
                .is_some_and(|message| { message.starts_with("invalid IPC request payload:") })
        );
        assert!(serialized["correlationId"].is_string());
        assert_eq!(serialized["retryable"], false);
    }
}
