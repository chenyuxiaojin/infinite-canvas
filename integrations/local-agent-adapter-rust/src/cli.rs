use std::{
    io::{self, Read},
    path::{Path, PathBuf},
};

use clap::{Args, Parser, Subcommand};
use serde_json::{json, Value};

use crate::{
    read_credential_token, AgentOperationRequest, BridgeClient, BridgeError, ProjectCreateRequest,
    TestClipRequest, VideoIngestRequest,
};

#[derive(Debug, Parser)]
#[command(
    name = "infinite-canvas",
    version,
    about = "Controlled local Agent client for Infinite Canvas"
)]
pub struct Cli {
    #[arg(long, default_value = "http://127.0.0.1:3102", global = true)]
    pub endpoint: String,
    #[arg(long, global = true)]
    pub credential_file: Option<PathBuf>,
    #[arg(long, global = true)]
    pub pretty: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Capabilities,
    Projects(ProjectsArgs),
    Canvas(CanvasArgs),
    Media(MediaArgs),
    Tasks(TasksArgs),
    Runtime,
    Credentials(CredentialsArgs),
}

#[derive(Debug, Args)]
pub struct ProjectsArgs {
    #[command(subcommand)]
    pub command: ProjectsCommand,
}

#[derive(Debug, Subcommand)]
pub enum ProjectsCommand {
    List,
    Get { project_id: String },
    Create(InputFile),
}

#[derive(Debug, Args)]
pub struct MediaArgs {
    #[command(subcommand)]
    pub command: MediaCommand,
}

#[derive(Debug, Subcommand)]
pub enum MediaCommand {
    Inbox,
    Video(MediaVideoArgs),
}

#[derive(Debug, Args)]
pub struct MediaVideoArgs {
    #[command(subcommand)]
    pub command: MediaVideoCommand,
}

#[derive(Debug, Subcommand)]
pub enum MediaVideoCommand {
    Ingest(InputFile),
}

#[derive(Debug, Args)]
pub struct CanvasArgs {
    #[command(subcommand)]
    pub command: CanvasCommand,
}

#[derive(Debug, Subcommand)]
pub enum CanvasCommand {
    Operations(OperationsArgs),
}

#[derive(Debug, Args)]
pub struct OperationsArgs {
    #[command(subcommand)]
    pub command: OperationsCommand,
}

#[derive(Debug, Subcommand)]
pub enum OperationsCommand {
    Apply(InputFile),
    DryRun(InputFile),
}

#[derive(Debug, Args)]
pub struct TasksArgs {
    #[command(subcommand)]
    pub command: TasksCommand,
}

#[derive(Debug, Subcommand)]
pub enum TasksCommand {
    Status { task_id: String },
    Cancel { task_id: String },
    TestClip(InputFile),
}

#[derive(Debug, Args)]
pub struct CredentialsArgs {
    #[command(subcommand)]
    pub command: CredentialsCommand,
}

#[derive(Debug, Subcommand)]
pub enum CredentialsCommand {
    Revoke,
}

#[derive(Debug, Args)]
pub struct InputFile {
    #[arg(long, value_name = "PATH_OR_DASH")]
    pub file: PathBuf,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(i32)]
pub enum ExitCode {
    Success = 0,
    Internal = 1,
    Usage = 2,
    Unavailable = 3,
    Unauthorized = 4,
    Conflict = 5,
    NotFound = 6,
    Rejected = 7,
}

pub fn run_cli(cli: Cli) -> ExitCode {
    let pretty = cli.pretty;
    match execute(cli) {
        Ok(value) => {
            print_json(&value, pretty);
            ExitCode::Success
        }
        Err(error) => {
            print_json(&json!(error.envelope()), pretty);
            exit_code(&error)
        }
    }
}

fn execute(cli: Cli) -> Result<Value, BridgeError> {
    let credential_path = match cli.credential_file {
        Some(path) => path,
        None => default_credential_path()?,
    };
    let token = read_credential_token(&credential_path)?;
    let client = BridgeClient::new(&cli.endpoint, token)?;
    match cli.command {
        Command::Capabilities => client.get("/v1/capabilities"),
        Command::Projects(args) => match args.command {
            ProjectsCommand::List => client.get("/v1/projects"),
            ProjectsCommand::Get { project_id } => {
                validate_route_identifier(&project_id)?;
                client.get(&format!("/v1/projects/{project_id}"))
            }
            ProjectsCommand::Create(input) => {
                let request = read_json::<ProjectCreateRequest>(&input.file)?;
                client.post("/v1/projects", &request)
            }
        },
        Command::Canvas(args) => match args.command {
            CanvasCommand::Operations(args) => match args.command {
                OperationsCommand::Apply(input) => {
                    let request = read_json::<AgentOperationRequest>(&input.file)?;
                    client.post("/v1/canvas/operations/apply", &request)
                }
                OperationsCommand::DryRun(input) => {
                    let request = read_json::<AgentOperationRequest>(&input.file)?;
                    client.post("/v1/canvas/operations/dry-run", &request)
                }
            },
        },
        Command::Media(args) => match args.command {
            MediaCommand::Inbox => client.get("/v1/media/inbox"),
            MediaCommand::Video(args) => match args.command {
                MediaVideoCommand::Ingest(input) => {
                    let request = read_json::<VideoIngestRequest>(&input.file)?;
                    client.post("/v1/media/video-ingests", &request)
                }
            },
        },
        Command::Tasks(args) => match args.command {
            TasksCommand::Status { task_id } => {
                validate_route_identifier(&task_id)?;
                client.get(&format!("/v1/tasks/{task_id}"))
            }
            TasksCommand::Cancel { task_id } => {
                validate_route_identifier(&task_id)?;
                client.post(&format!("/v1/tasks/{task_id}/cancel"), &json!({}))
            }
            TasksCommand::TestClip(input) => {
                let request = read_json::<TestClipRequest>(&input.file)?;
                client.post("/v1/tasks/test-clips", &request)
            }
        },
        Command::Runtime => client.get("/v1/runtime"),
        Command::Credentials(args) => match args.command {
            CredentialsCommand::Revoke => client.post("/v1/credentials/revoke", &json!({})),
        },
    }
}

fn default_credential_path() -> Result<PathBuf, BridgeError> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| BridgeError::invalid("The macOS home directory is unavailable."))?;
    Ok(home
        .join("Library")
        .join("Application Support")
        .join("com.chenyuxiaojin.infinitecanvas")
        .join("agent-bridge")
        .join("credential.json"))
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, BridgeError> {
    let mut bytes = Vec::new();
    if path == Path::new("-") {
        io::stdin()
            .take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| BridgeError::invalid("The JSON request could not be read from stdin."))?;
    } else {
        bytes = std::fs::read(path)
            .map_err(|_| BridgeError::invalid("The JSON request file could not be read."))?;
    }
    if bytes.len() > 1024 * 1024 {
        return Err(BridgeError::invalid(
            "The JSON request exceeds the 1 MiB limit.",
        ));
    }
    serde_json::from_slice(&bytes)
        .map_err(|_| BridgeError::invalid("The JSON request does not match the allowed schema."))
}

fn validate_route_identifier(value: &str) -> Result<(), BridgeError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return Err(BridgeError::invalid("The route identifier is invalid."));
    }
    Ok(())
}

fn exit_code(error: &BridgeError) -> ExitCode {
    match error.code {
        "UNAUTHORIZED" => ExitCode::Unauthorized,
        "STALE_REVISION"
        | "REVISION_CONFLICT"
        | "REQUEST_ID_REUSED"
        | "LOCKED_NODE"
        | "NODE_EXISTS"
        | "CONNECTION_EXISTS"
        | "PROJECT_DELETED"
        | "PROJECT_EXISTS"
        | "MEDIA_DIGEST_MISMATCH"
        | "TASK_CONFLICT" => ExitCode::Conflict,
        "NOT_FOUND" | "CAPABILITY_NOT_FOUND" => ExitCode::NotFound,
        "INVALID_REQUEST" => ExitCode::Usage,
        "RUNTIME_UNAVAILABLE" => ExitCode::Unavailable,
        "CAPABILITY_DENIED" | "METHOD_NOT_ALLOWED" => ExitCode::Rejected,
        _ => ExitCode::Internal,
    }
}

fn print_json(value: &Value, pretty: bool) {
    let encoded = if pretty {
        serde_json::to_string_pretty(value)
    } else {
        serde_json::to_string(value)
    }
    .unwrap_or_else(|_| {
        "{\"ok\":false,\"error\":{\"code\":\"INTERNAL\",\"message\":\"JSON encoding failed\"}}"
            .to_owned()
    });
    println!("{encoded}");
}
