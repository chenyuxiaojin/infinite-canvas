mod auth;
mod canvas;
mod capabilities;
mod cli;
mod client;
mod error;
pub mod history;
mod mcp;
mod project_binding;
mod runtime;
mod server;

pub use auth::{read_credential_token, CredentialDocument, CredentialStore};
pub use canvas::{
    Actor, AgentOperationRequest, CanvasOperation, CanvasOperationAdapter, CanvasOperationResult,
    ProjectDocument, ProjectSummary, SqliteCanvasAdapter,
};
pub use cli::{run_cli, Cli, ExitCode};
pub use client::BridgeClient;
pub use error::{BridgeError, ErrorBody, ErrorEnvelope};
pub use mcp::serve_mcp_stdio;
pub use project_binding::{
    find_project_binding, load_project_binding, setup_project_binding, ProjectBinding,
};
pub use runtime::{AgentRuntime, TestClipRequest};
pub use server::{BridgeServer, BRIDGE_PORT};
