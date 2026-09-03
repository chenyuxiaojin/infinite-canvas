use std::{
    io::{self, Read},
    path::{Path, PathBuf},
};

use clap::{Args, Parser, Subcommand};
use serde_json::{json, Value};

use crate::{
    read_credential_token, serve_mcp_stdio, setup_project_binding, AgentOperationRequest,
    BridgeClient, BridgeError, TestClipRequest,
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
    Tasks(TasksArgs),
    Runtime,
    Agents(AgentsArgs),
    Mcp(McpArgs),
    Credentials(CredentialsArgs),
}

#[derive(Debug, Args)]
pub struct AgentsArgs {
    #[command(subcommand)]
    pub command: AgentsCommand,
}

#[derive(Debug, Subcommand)]
pub enum AgentsCommand {
    Setup {
        #[arg(long)]
        project_dir: PathBuf,
        #[arg(long)]
        canvas_project_id: String,
        #[arg(long, default_value = "")]
        canvas_project_title: String,
    },
}

#[derive(Debug, Args)]
pub struct McpArgs {
    #[command(subcommand)]
    pub command: McpCommand,
}

#[derive(Debug, Subcommand)]
pub enum McpCommand {
    Serve {
        #[arg(long)]
        project_dir: Option<PathBuf>,
    },
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
    let credential_path = match cli.credential_file {
        Some(path) => Ok(path),
        None => default_credential_path(),
    };
    let result = match cli.command {
        Command::Mcp(args) => {
            let credential_path = match credential_path {
                Ok(path) => path,
                Err(error) => return print_error_and_code(error, pretty),
            };
            let result = match args.command {
                McpCommand::Serve { project_dir } => {
                    serve_mcp_stdio(&cli.endpoint, &credential_path, project_dir.as_deref())
                }
            };
            return match result {
                Ok(()) => ExitCode::Success,
                Err(error) => {
                    eprintln!(
                        "{}",
                        serde_json::to_string(&error.envelope())
                            .unwrap_or_else(|_| "MCP server failed".to_owned())
                    );
                    exit_code(&error)
                }
            };
        }
        Command::Agents(args) => match args.command {
            AgentsCommand::Setup {
                project_dir,
                canvas_project_id,
                canvas_project_title,
            } => {
                let executable = std::env::current_exe().map_err(|_| {
                    BridgeError::internal("The Infinite Canvas command path is unavailable.")
                });
                executable.and_then(|executable| {
                    setup_project_binding(
                        &project_dir,
                        &canvas_project_id,
                        &canvas_project_title,
                        &executable,
                    )
                    .and_then(|binding| {
                        serde_json::to_value(binding).map_err(|_| {
                            BridgeError::internal("The project binding could not be encoded.")
                        })
                    })
                })
            }
        },
        command => {
            let credential_path = match credential_path {
                Ok(path) => path,
                Err(error) => return print_error_and_code(error, pretty),
            };
            execute_bridge(&cli.endpoint, &credential_path, command)
        }
    };
    match result {
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

fn execute_bridge(
    endpoint: &str,
    credential_path: &Path,
    command: Command,
) -> Result<Value, BridgeError> {
    let token = read_credential_token(credential_path)?;
    let client = BridgeClient::new(endpoint, token)?;
    match command {
        Command::Capabilities => client.get("/v1/capabilities"),
        Command::Projects(args) => match args.command {
            ProjectsCommand::List => client.get("/v1/projects"),
            ProjectsCommand::Get { project_id } => {
                validate_route_identifier(&project_id)?;
                client.get(&format!("/v1/projects/{project_id}"))
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
        Command::Agents(_) | Command::Mcp(_) => Err(BridgeError::internal(
            "The local command was routed incorrectly.",
        )),
        Command::Credentials(args) => match args.command {
            CredentialsCommand::Revoke => client.post("/v1/credentials/revoke", &json!({})),
        },
    }
}

fn print_error_and_code(error: BridgeError, pretty: bool) -> ExitCode {
    print_json(&json!(error.envelope()), pretty);
    exit_code(&error)
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
        "REVISION_CONFLICT" | "REQUEST_ID_REUSED" | "NODE_EXISTS" | "CONNECTION_EXISTS"
        | "PROJECT_DELETED" => ExitCode::Conflict,
        "NOT_FOUND" | "CAPABILITY_NOT_FOUND" | "NO_PROJECT_BINDING" => ExitCode::NotFound,
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
