use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::BridgeError;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TestClipRequest {
    pub project_id: String,
    pub request_id: String,
    pub base_revision: String,
    pub actor: crate::Actor,
}

pub trait AgentRuntime: Send + Sync {
    fn report(&self) -> Result<Value, BridgeError>;
    fn submit_test_clip(&self, request: &TestClipRequest) -> Result<Value, BridgeError>;
    fn task_status(&self, task_id: &str) -> Result<Value, BridgeError>;
    fn cancel_task(&self, task_id: &str) -> Result<Value, BridgeError>;
}
