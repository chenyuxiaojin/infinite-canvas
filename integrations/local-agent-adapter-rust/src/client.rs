use std::time::Duration;

use http::StatusCode;
use serde::Serialize;
use serde_json::Value;
use url::Url;

use crate::{BridgeError, ErrorEnvelope};

pub struct BridgeClient {
    endpoint: String,
    token: String,
    agent: ureq::Agent,
}

impl BridgeClient {
    pub fn new(endpoint: &str, token: String) -> Result<Self, BridgeError> {
        let endpoint = validate_endpoint(endpoint)?;
        if token.trim().is_empty() {
            return Err(BridgeError::unauthorized());
        }
        Ok(Self {
            endpoint,
            token,
            agent: ureq::AgentBuilder::new()
                .redirects(0)
                .timeout(Duration::from_secs(30))
                .build(),
        })
    }

    pub fn get(&self, path: &str) -> Result<Value, BridgeError> {
        let response = self
            .agent
            .get(&self.url(path)?)
            .set("Authorization", &format!("Bearer {}", self.token))
            .call();
        decode_response(response)
    }

    pub fn post<T: Serialize>(&self, path: &str, body: &T) -> Result<Value, BridgeError> {
        let response = self
            .agent
            .post(&self.url(path)?)
            .set("Authorization", &format!("Bearer {}", self.token))
            .send_json(body);
        decode_response(response)
    }

    fn url(&self, path: &str) -> Result<String, BridgeError> {
        if !path.starts_with('/') || path.contains("..") || path.contains(['\r', '\n']) {
            return Err(BridgeError::invalid("The Agent Bridge route is invalid."));
        }
        Ok(format!("{}{}", self.endpoint, path))
    }
}

fn validate_endpoint(endpoint: &str) -> Result<String, BridgeError> {
    let parsed = Url::parse(endpoint)
        .map_err(|_| BridgeError::invalid("The Agent Bridge endpoint is invalid."))?;
    if parsed.scheme() != "http"
        || parsed.host_str() != Some("127.0.0.1")
        || parsed.port().is_none()
        || (parsed.path() != "/" && !parsed.path().is_empty())
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err(BridgeError::forbidden(
            "Agent credentials may only be sent to an explicit 127.0.0.1 HTTP endpoint.",
        ));
    }
    Ok(endpoint.trim_end_matches('/').to_owned())
}

fn decode_response(response: Result<ureq::Response, ureq::Error>) -> Result<Value, BridgeError> {
    match response {
        Ok(response) => {
            let value = response
                .into_json::<Value>()
                .map_err(|_| BridgeError::internal("The Agent Bridge returned invalid JSON."))?;
            if value.get("ok").and_then(Value::as_bool) != Some(true) {
                return Err(BridgeError::internal(
                    "The Agent Bridge returned an invalid success envelope.",
                ));
            }
            Ok(value)
        }
        Err(ureq::Error::Status(status, response)) => {
            let envelope = response.into_json::<ErrorEnvelope>().ok();
            let code = envelope
                .as_ref()
                .map(|value| value.error.code.as_str())
                .unwrap_or("BRIDGE_ERROR")
                .to_owned();
            let message = envelope
                .as_ref()
                .map(|value| value.error.message.clone())
                .unwrap_or_else(|| "The Agent Bridge rejected the request.".to_owned());
            let details = envelope.and_then(|value| value.error.details);
            let mut error = BridgeError::new(
                known_error_code(&code),
                StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                message,
            );
            error.details = details;
            Err(error)
        }
        Err(ureq::Error::Transport(_)) => Err(BridgeError::unavailable(
            "The Infinite Canvas desktop Agent Bridge is not running.",
        )),
    }
}

fn known_error_code(code: &str) -> &'static str {
    match code {
        "UNAUTHORIZED" => "UNAUTHORIZED",
        "INVALID_REQUEST" => "INVALID_REQUEST",
        "CAPABILITY_DENIED" => "CAPABILITY_DENIED",
        "NOT_FOUND" => "NOT_FOUND",
        "STALE_REVISION" => "STALE_REVISION",
        "REVISION_CONFLICT" => "REVISION_CONFLICT",
        "REQUEST_ID_REUSED" => "REQUEST_ID_REUSED",
        "LOCKED_NODE" => "LOCKED_NODE",
        "NODE_EXISTS" => "NODE_EXISTS",
        "CONNECTION_EXISTS" => "CONNECTION_EXISTS",
        "PROJECT_DELETED" => "PROJECT_DELETED",
        "PROJECT_EXISTS" => "PROJECT_EXISTS",
        "MEDIA_DIGEST_MISMATCH" => "MEDIA_DIGEST_MISMATCH",
        "MEDIA_REFERENCE_UNAVAILABLE" => "MEDIA_REFERENCE_UNAVAILABLE",
        "TASK_CONFLICT" => "TASK_CONFLICT",
        "CAPABILITY_NOT_FOUND" => "CAPABILITY_NOT_FOUND",
        "METHOD_NOT_ALLOWED" => "METHOD_NOT_ALLOWED",
        "RUNTIME_UNAVAILABLE" => "RUNTIME_UNAVAILABLE",
        _ => "BRIDGE_ERROR",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_cannot_redirect_a_credential_to_non_loopback() {
        for endpoint in [
            "http://localhost:3102",
            "http://0.0.0.0:3102",
            "https://127.0.0.1:3102",
            "http://example.com:3102",
            "http://127.0.0.1:3102/v1",
        ] {
            assert!(BridgeClient::new(endpoint, "fixture".to_owned()).is_err());
        }
    }
}
