use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::BridgeError;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TestClipRequest {
    pub project_id: String,
    pub node_id: String,
    pub request_id: String,
    #[serde(deserialize_with = "read_runtime_revision")]
    pub base_revision: String,
    pub actor: crate::Actor,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VideoIngestRequest {
    pub project_id: String,
    pub node_id: String,
    pub request_id: String,
    #[serde(deserialize_with = "read_runtime_revision")]
    pub base_revision: String,
    pub actor: crate::Actor,
    pub inbox_file_name: String,
    pub expected_sha256: String,
    pub title: String,
    pub position: crate::Point,
    pub size: crate::CanvasSize,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ImageIngestRequest {
    pub project_id: String,
    pub node_id: String,
    pub request_id: String,
    #[serde(deserialize_with = "read_runtime_revision")]
    pub base_revision: String,
    pub actor: crate::Actor,
    pub inbox_file_name: String,
    pub expected_sha256: String,
    pub title: String,
    pub position: crate::Point,
    pub size: crate::CanvasSize,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VideoGenerationRequest {
    pub project_id: String,
    pub node_id: String,
    pub request_id: String,
    #[serde(deserialize_with = "read_runtime_revision")]
    pub base_revision: String,
    pub actor: crate::Actor,
    pub title: String,
    pub prompt: String,
    pub image_node_id: String,
    pub resolution: String,
    pub duration_seconds: u64,
    pub position: crate::Point,
    pub size: crate::CanvasSize,
}

pub trait AgentRuntime: Send + Sync {
    fn report(&self) -> Result<Value, BridgeError>;
    fn media_inbox(&self) -> Result<Value, BridgeError> { Err(BridgeError::unavailable("Media capability unavailable")) }
    fn validate_video_ingest(&self, request: &VideoIngestRequest) -> Result<(), BridgeError> { Err(BridgeError::unavailable("Media capability unavailable")) }
    fn submit_video_ingest(&self, request: &VideoIngestRequest) -> Result<Value, BridgeError> { Err(BridgeError::unavailable("Media capability unavailable")) }
    fn ingest_image(&self, request: &ImageIngestRequest) -> Result<Value, BridgeError> { Err(BridgeError::unavailable("Media capability unavailable")) }
    fn verify_media_reference(&self, reference: &Value) -> Result<(), BridgeError> { Err(BridgeError::unavailable("Media capability unavailable")) }
    fn quote_video_generation(
        &self,
        resolution: &str,
        duration_seconds: u64,
    ) -> Result<Value, BridgeError> { Err(BridgeError::unavailable("Media capability unavailable")) }
    fn submit_test_clip(&self, request: &TestClipRequest) -> Result<Value, BridgeError>;
    fn task_status(&self, task_id: &str) -> Result<Value, BridgeError>;
    fn cancel_task(&self, task_id: &str) -> Result<Value, BridgeError>;
}

fn read_runtime_revision<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let value = Value::deserialize(deserializer)?;
    match value { Value::String(value) => Ok(value), Value::Number(value) if value.is_u64() => Ok(value.to_string()), _ => Err(serde::de::Error::custom("invalid canvas revision")) }
}
