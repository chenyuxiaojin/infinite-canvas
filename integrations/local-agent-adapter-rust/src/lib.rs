mod auth;
mod canvas;
mod capabilities;
mod cli;
mod client;
mod error;
mod runtime;
mod server;

pub use auth::{read_credential_token, CredentialDocument, CredentialStore};
pub use canvas::{
    Actor, AgentOperationRequest, CanonicalCanvasAdapter, CanvasOperation, CanvasOperationAdapter,
    CanvasOperationResult, CanvasProtocolExecutor, CanvasRuntimeTaskReference, CanvasSize,
    HttpCanvasProtocolExecutor, Point, ProjectCreateRequest, ProjectCreateResult, ProjectDocument,
    ProjectSummary, ProtocolOutcome,
};
pub use cli::{run_cli, Cli, ExitCode};
pub use client::BridgeClient;
pub use error::{BridgeError, ErrorBody, ErrorEnvelope};
pub use runtime::{AgentRuntime, ImageIngestRequest, TestClipRequest, VideoIngestRequest};
pub use server::{BridgeServer, BRIDGE_PORT};
