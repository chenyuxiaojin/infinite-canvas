use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use toml_edit::{value, Array, DocumentMut, Item, Table};

use crate::BridgeError;

const BINDING_DIRECTORY: &str = ".infinite-canvas";
const BINDING_FILE: &str = "project.json";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectBinding {
    pub version: u8,
    pub project_id: String,
    pub project_title: String,
    pub project_directory: String,
}

impl ProjectBinding {
    pub fn new(
        project_id: &str,
        project_title: &str,
        project_directory: &Path,
    ) -> Result<Self, BridgeError> {
        validate_project_id(project_id)?;
        let canonical = project_directory
            .canonicalize()
            .map_err(|_| BridgeError::invalid("The selected film directory does not exist."))?;
        if !canonical.is_dir() {
            return Err(BridgeError::invalid(
                "The selected film path is not a directory.",
            ));
        }
        Ok(Self {
            version: 1,
            project_id: project_id.to_owned(),
            project_title: project_title.trim().to_owned(),
            project_directory: canonical.to_string_lossy().into_owned(),
        })
    }
}

pub fn setup_project_binding(
    project_directory: &Path,
    project_id: &str,
    project_title: &str,
    cli_path: &Path,
) -> Result<ProjectBinding, BridgeError> {
    let binding = ProjectBinding::new(project_id, project_title, project_directory)?;
    let directory = PathBuf::from(&binding.project_directory);
    let canonical_cli = cli_path
        .canonicalize()
        .map_err(|_| BridgeError::invalid("The Infinite Canvas Agent command is unavailable."))?;

    let binding_directory = directory.join(BINDING_DIRECTORY);
    fs::create_dir_all(&binding_directory)
        .map_err(|_| BridgeError::internal("The film binding directory could not be created."))?;
    write_json_atomic(&binding_directory.join(BINDING_FILE), &json!(binding))?;
    update_claude_config(&directory.join(".mcp.json"), &canonical_cli, &directory)?;
    update_codex_config(
        &directory.join(".codex").join("config.toml"),
        &canonical_cli,
        &directory,
    )?;
    Ok(binding)
}

pub fn load_project_binding(project_directory: &Path) -> Result<ProjectBinding, BridgeError> {
    let path = project_directory.join(BINDING_DIRECTORY).join(BINDING_FILE);
    let bytes = fs::read(&path).map_err(|_| {
        BridgeError::no_project_binding(format!(
            "No canvas is bound to this film directory. Run `infinite-canvas agents setup --project-dir \"{}\" --canvas-project-id <id> --canvas-project-title <title>` first.",
            project_directory.display()
        ))
    })?;
    let binding: ProjectBinding = serde_json::from_slice(&bytes)
        .map_err(|_| BridgeError::invalid("The film canvas binding is invalid."))?;
    if binding.version != 1 {
        return Err(BridgeError::invalid(
            "The film canvas binding version is unsupported.",
        ));
    }
    validate_project_id(&binding.project_id)?;
    Ok(binding)
}

pub fn find_project_binding(
    start: Option<&Path>,
) -> Result<(PathBuf, ProjectBinding), BridgeError> {
    let start = if let Some(path) = start {
        path.to_path_buf()
    } else if let Some(path) = std::env::var_os("INFINITE_CANVAS_PROJECT_DIR") {
        PathBuf::from(path)
    } else if let Some(path) = std::env::var_os("CLAUDE_PROJECT_DIR") {
        PathBuf::from(path)
    } else {
        std::env::current_dir().map_err(|_| {
            BridgeError::no_project_binding("The current film directory is unavailable.")
        })?
    };

    let canonical = start.canonicalize().map_err(|_| {
        BridgeError::no_project_binding("The current film directory does not exist.")
    })?;
    for directory in canonical.ancestors() {
        let path = directory.join(BINDING_DIRECTORY).join(BINDING_FILE);
        if path.is_file() {
            return Ok((directory.to_path_buf(), load_project_binding(directory)?));
        }
    }
    Err(BridgeError::no_project_binding(
        "This AI session is not inside a film directory bound to Infinite Canvas.",
    ))
}

fn validate_project_id(value: &str) -> Result<(), BridgeError> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return Err(BridgeError::invalid("The canvas project id is invalid."));
    }
    Ok(())
}

fn update_claude_config(
    path: &Path,
    cli_path: &Path,
    project_directory: &Path,
) -> Result<(), BridgeError> {
    let parent_path = project_directory
        .parent()
        .map(|parent| parent.join(".mcp.json"));
    let mut root = parent_path
        .as_deref()
        .filter(|parent| parent.is_file())
        .map(read_claude_config)
        .transpose()?
        .unwrap_or_else(|| json!({}));

    if path.exists() {
        let child = read_claude_config(path)?;
        merge_claude_config(&mut root, child)?;
    }
    let object = root
        .as_object_mut()
        .ok_or_else(|| BridgeError::invalid("The existing .mcp.json must contain an object."))?;
    let servers = object
        .entry("mcpServers".to_owned())
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| {
            BridgeError::invalid("The existing mcpServers value must contain an object.")
        })?;
    servers.insert(
        "infinite-canvas".to_owned(),
        json!({
            "type": "stdio",
            "command": cli_path.to_string_lossy(),
            "args": ["mcp", "serve"],
            "env": { "INFINITE_CANVAS_PROJECT_DIR": project_directory.to_string_lossy() }
        }),
    );
    write_json_atomic(path, &root)
}

fn read_claude_config(path: &Path) -> Result<Value, BridgeError> {
    serde_json::from_slice::<Value>(&fs::read(path).map_err(|_| {
        BridgeError::internal("The existing Claude MCP configuration could not be read.")
    })?)
    .map_err(|_| BridgeError::invalid("The existing .mcp.json is not valid JSON."))
}

fn merge_claude_config(base: &mut Value, overlay: Value) -> Result<(), BridgeError> {
    let base_object = base
        .as_object_mut()
        .ok_or_else(|| BridgeError::invalid("The existing .mcp.json must contain an object."))?;
    let overlay_object = overlay
        .as_object()
        .ok_or_else(|| BridgeError::invalid("The existing .mcp.json must contain an object."))?;

    for (key, value) in overlay_object {
        if key != "mcpServers" {
            base_object.insert(key.clone(), value.clone());
        }
    }

    if let Some(overlay_servers) = overlay_object.get("mcpServers") {
        let overlay_servers = overlay_servers.as_object().ok_or_else(|| {
            BridgeError::invalid("The existing mcpServers value must contain an object.")
        })?;
        let base_servers = base_object
            .entry("mcpServers".to_owned())
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .ok_or_else(|| {
                BridgeError::invalid("The existing mcpServers value must contain an object.")
            })?;
        for (name, server) in overlay_servers {
            base_servers.insert(name.clone(), server.clone());
        }
    }
    Ok(())
}

fn update_codex_config(
    path: &Path,
    cli_path: &Path,
    project_directory: &Path,
) -> Result<(), BridgeError> {
    let mut document = if path.exists() {
        fs::read_to_string(path)
            .map_err(|_| {
                BridgeError::internal("The existing Codex MCP configuration could not be read.")
            })?
            .parse::<DocumentMut>()
            .map_err(|_| {
                BridgeError::invalid("The existing .codex/config.toml is not valid TOML.")
            })?
    } else {
        DocumentMut::new()
    };

    let mut server = Table::new();
    server["command"] = value(cli_path.to_string_lossy().into_owned());
    let mut args = Array::new();
    args.push("mcp");
    args.push("serve");
    server["args"] = value(args);
    server["enabled"] = value(true);
    let mut environment = Table::new();
    environment.set_implicit(true);
    environment["INFINITE_CANVAS_PROJECT_DIR"] =
        value(project_directory.to_string_lossy().into_owned());
    server["env"] = Item::Table(environment);
    if document.get("mcp_servers").is_none() {
        document["mcp_servers"] = Item::Table(Table::new());
    } else if !document.get("mcp_servers").is_some_and(Item::is_table) {
        return Err(BridgeError::invalid(
            "The existing mcp_servers value in .codex/config.toml must be a table.",
        ));
    }
    document
        .get_mut("mcp_servers")
        .expect("mcp_servers was just initialized")
        .as_table_mut()
        .expect("mcp_servers was just initialized as a table")
        .insert("infinite_canvas", Item::Table(server));

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|_| {
            BridgeError::internal("The Codex project configuration directory could not be created.")
        })?;
    }
    write_bytes_atomic(path, document.to_string().as_bytes())
}

fn write_json_atomic(path: &Path, value: &Value) -> Result<(), BridgeError> {
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|_| BridgeError::internal("The project binding could not be encoded."))?;
    write_bytes_atomic(path, &bytes)
}

fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> Result<(), BridgeError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|_| {
            BridgeError::internal("The project configuration directory could not be created.")
        })?;
    }
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    fs::write(&temporary, bytes)
        .map_err(|_| BridgeError::internal("The project configuration could not be staged."))?;
    fs::rename(&temporary, path)
        .map_err(|_| BridgeError::internal("The project configuration could not be published."))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_preserves_existing_claude_servers_and_creates_both_agent_configs() {
        let root = tempfile::tempdir().unwrap();
        let cli = root.path().join("infinite-canvas");
        fs::write(&cli, b"fixture").unwrap();
        fs::write(
            root.path().join(".mcp.json"),
            br#"{"mcpServers":{"existing":{"command":"example"}}}"#,
        )
        .unwrap();

        let binding = setup_project_binding(root.path(), "project-1", "Film one", &cli).unwrap();
        assert_eq!(binding.project_id, "project-1");
        let claude: Value =
            serde_json::from_slice(&fs::read(root.path().join(".mcp.json")).unwrap()).unwrap();
        assert_eq!(claude["mcpServers"]["existing"]["command"], "example");
        assert_eq!(claude["mcpServers"]["infinite-canvas"]["args"][0], "mcp");
        let codex = fs::read_to_string(root.path().join(".codex/config.toml")).unwrap();
        assert!(codex.contains("[mcp_servers.infinite_canvas]"));
        assert_eq!(
            load_project_binding(root.path()).unwrap().project_title,
            "Film one"
        );
    }

    #[test]
    fn binding_is_found_from_a_nested_working_directory() {
        let root = tempfile::tempdir().unwrap();
        let nested = root.path().join("shots/scene-1");
        fs::create_dir_all(&nested).unwrap();
        fs::create_dir(root.path().join(BINDING_DIRECTORY)).unwrap();
        write_json_atomic(
            &root.path().join(BINDING_DIRECTORY).join(BINDING_FILE),
            &json!(ProjectBinding::new("project-1", "Film", root.path()).unwrap()),
        )
        .unwrap();
        let (directory, binding) = find_project_binding(Some(&nested)).unwrap();
        assert_eq!(directory, root.path().canonicalize().unwrap());
        assert_eq!(binding.project_id, "project-1");
    }

    #[test]
    fn setup_inherits_parent_claude_servers_without_overwriting_child_servers() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("film-one");
        fs::create_dir(&project).unwrap();
        let cli = root.path().join("infinite-canvas");
        fs::write(&cli, b"fixture").unwrap();
        fs::write(
            root.path().join(".mcp.json"),
            br#"{"mcpServers":{"parent-server":{"command":"parent"}}}"#,
        )
        .unwrap();
        fs::write(
            project.join(".mcp.json"),
            br#"{"mcpServers":{"child-server":{"command":"child"}}}"#,
        )
        .unwrap();

        setup_project_binding(&project, "project-1", "Film one", &cli).unwrap();

        let claude: Value =
            serde_json::from_slice(&fs::read(project.join(".mcp.json")).unwrap()).unwrap();
        assert_eq!(claude["mcpServers"]["parent-server"]["command"], "parent");
        assert_eq!(claude["mcpServers"]["child-server"]["command"], "child");
        assert_eq!(claude["mcpServers"]["infinite-canvas"]["args"][0], "mcp");
    }
}
