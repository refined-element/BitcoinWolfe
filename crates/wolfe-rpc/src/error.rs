use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum RpcError {
    #[error("method not found: {0}")]
    MethodNotFound(String),

    #[error("invalid params: {0}")]
    InvalidParams(String),

    #[error("internal error: {0}")]
    Internal(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("wallet error: {0}")]
    Wallet(String),

    #[error("lightning error: {0}")]
    Lightning(String),
}

impl IntoResponse for RpcError {
    fn into_response(self) -> Response {
        let (status, code, message) = match &self {
            RpcError::MethodNotFound(m) => (StatusCode::NOT_FOUND, -32601, m.clone()),
            RpcError::InvalidParams(m) => (StatusCode::BAD_REQUEST, -32602, m.clone()),
            RpcError::Internal(m) => (StatusCode::INTERNAL_SERVER_ERROR, -32603, m.clone()),
            RpcError::NotFound(m) => (StatusCode::NOT_FOUND, -1, m.clone()),
            RpcError::Wallet(m) => (StatusCode::INTERNAL_SERVER_ERROR, -4, m.clone()),
            RpcError::Lightning(m) => (StatusCode::INTERNAL_SERVER_ERROR, -5, m.clone()),
        };

        let body = json!({
            "error": {
                "code": code,
                "message": message,
            }
        });

        (status, axum::Json(body)).into_response()
    }
}
