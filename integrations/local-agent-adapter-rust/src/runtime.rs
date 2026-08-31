use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::BridgeError;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TestClipRequest {
    pub project_id: String,
    pub node_id: String,
    pub request_id: String,
    pub base_revision: u64,
    pub actor: crate::Actor,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VideoIngestRequest {
    pub project_id: String,
    pub node_id: String,
    pub request_id: String,
    pub base_revision: u64,
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
    pub base_revision: u64,
    pub actor: crate::Actor,
    pub inbox_file_name: String,
    pub expected_sha256: String,
    pub title: String,
    pub position: crate::Point,
    pub size: crate::CanvasSize,
}

pub trait AgentRuntime: Send + Sync {
    fn report(&self) -> Result<Value, BridgeError>;
    fn media_inbox(&self) -> Result<Value, BridgeError>;
    fn validate_video_ingest(&self, request: &VideoIngestRequest) -> Result<(), BridgeError>;
    fn submit_video_ingest(&self, request: &VideoIngestRequest) -> Result<Value, BridgeError>;
    fn ingest_image(&self, request: &ImageIngestRequest) -> Result<Value, BridgeError>;
    fn submit_test_clip(&self, request: &TestClipRequest) -> Result<Value, BridgeError>;
    fn task_status(&self, task_id: &str) -> Result<Value, BridgeError>;
    fn cancel_task(&self, task_id: &str) -> Result<Value, BridgeError>;
}
