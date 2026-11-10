use thiserror::Error;

/// Errors produced by the rustman core library.
#[derive(Error, Debug)]
pub enum RustmanError {
    #[error("network error: {0}")]
    NetworkError(String),
    #[error("serialization error: {0}")]
    SerializationError(String),
    #[error("invalid URL: {0}")]
    InvalidUrl(String),
    #[error("invalid header: {0}")]
    InvalidHeader(String),
    #[error("request timed out")]
    Timeout,
    #[error("invalid request: {0}")]
    InvalidRequest(String),
}

impl From<reqwest::Error> for RustmanError {
    fn from(e: reqwest::Error) -> Self {
        if e.is_timeout() {
            RustmanError::Timeout
        } else if e.is_builder() {
            RustmanError::InvalidRequest(e.to_string())
        } else {
            RustmanError::NetworkError(e.to_string())
        }
    }
}

impl From<serde_json::Error> for RustmanError {
    fn from(e: serde_json::Error) -> Self {
        RustmanError::SerializationError(e.to_string())
    }
}
