use crate::dto::WebAppErrorResponse;
use aelf_crypto::CryptoError;
use std::fmt;
use thiserror::Error;

#[derive(Debug, Error)]
#[error("{message}")]
pub struct RequestError {
    pub message: String,
    pub request_id: Option<String>,
    pub endpoint: Option<String>,
    pub status: Option<u16>,
    pub chain_code: Option<String>,
    pub details: Option<String>,
}

#[derive(Debug, Error)]
pub enum AElfError {
    #[error(transparent)]
    Request(Box<RequestError>),
    #[error("invalid config: {0}")]
    InvalidConfig(String),
    #[error("missing field: {0}")]
    MissingField(&'static str),
    #[error("crypto error: {0}")]
    Crypto(#[from] CryptoError),
    #[error("hex error: {0}")]
    Hex(#[from] hex::FromHexError),
    #[error("base64 decode error: {0}")]
    Base64(#[from] base64::DecodeError),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("protobuf encode error: {0}")]
    ProtobufEncode(#[from] prost::EncodeError),
    #[error("protobuf decode error: {0}")]
    ProtobufDecode(#[from] prost::DecodeError),
}

impl AElfError {
    pub fn request(message: impl Into<String>, endpoint: Option<String>) -> Self {
        Self::Request(Box::new(RequestError {
            message: message.into(),
            request_id: None,
            endpoint,
            status: None,
            chain_code: None,
            details: None,
        }))
    }

    pub(crate) fn from_response(
        endpoint: String,
        status: reqwest::StatusCode,
        request_id: Option<String>,
        raw_body: &str,
    ) -> Self {
        let parsed = serde_json::from_str::<WebAppErrorResponse>(raw_body).ok();
        let (message, chain_code, details) = match parsed {
            Some(error) => (
                error.error.message,
                Some(error.error.code),
                error.error.details,
            ),
            None => (
                format!("request failed with status {}: {raw_body}", status),
                None,
                None,
            ),
        };

        Self::Request(Box::new(RequestError {
            message,
            request_id,
            endpoint: Some(endpoint),
            status: Some(status.as_u16()),
            chain_code,
            details,
        }))
    }
}

impl fmt::Display for WebAppErrorResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.error.message, self.error.code)
    }
}
