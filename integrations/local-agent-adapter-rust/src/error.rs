use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ErrorEnvelope {
    pub ok: bool,
    pub error: ErrorBody,
}

#[derive(Clone, Debug, Error)]
#[error("{message}")]
pub struct BridgeError {
    pub code: &'static str,
    pub message: String,
    pub status: u16,
    pub details: Option<Value>,
}

impl BridgeError {
    pub fn new(code: &'static str, status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            status: status.as_u16(),
            details: None,
        }
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new("INVALID_REQUEST", StatusCode::BAD_REQUEST, message)
    }

    pub fn unauthorized() -> Self {
        Self::new(
            "UNAUTHORIZED",
            StatusCode::UNAUTHORIZED,
            "A valid local Agent credential is required.",
        )
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new("CAPABILITY_DENIED", StatusCode::FORBIDDEN, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new("NOT_FOUND", StatusCode::NOT_FOUND, message)
    }

    pub fn conflict(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(code, StatusCode::CONFLICT, message)
    }

    pub fn unavailable(message: impl Into<String>) -> Self {
        Self::new(
            "RUNTIME_UNAVAILABLE",
            StatusCode::SERVICE_UNAVAILABLE,
            message,
        )
    }

    pub fn no_project_binding(message: impl Into<String>) -> Self {
        Self::new("NO_PROJECT_BINDING", StatusCode::NOT_FOUND, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new("INTERNAL", StatusCode::INTERNAL_SERVER_ERROR, message)
    }

    pub fn with_details(mut self, details: Value) -> Self {
        self.details = Some(details);
        self
    }

    pub fn status_code(&self) -> StatusCode {
        StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
    }

    pub fn envelope(&self) -> ErrorEnvelope {
        ErrorEnvelope {
            ok: false,
            error: ErrorBody {
                code: self.code.to_owned(),
                message: self.message.clone(),
                details: self.details.clone(),
            },
        }
    }
}

impl IntoResponse for BridgeError {
    fn into_response(self) -> Response {
        (self.status_code(), Json(self.envelope())).into_response()
    }
}

impl From<rusqlite::Error> for BridgeError {
    fn from(_: rusqlite::Error) -> Self {
        Self::internal("The desktop canvas store could not complete the request.")
    }
}
