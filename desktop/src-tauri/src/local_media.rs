use std::{
    collections::{HashMap, HashSet},
    fs::{File, OpenOptions},
    io::{BufReader, Read, Seek, Write},
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener},
    path::{Component, Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex, RwLock},
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    body::Body,
    extract::{Path as AxumPath, Query, State},
    http::{
        header::{
            ACCEPT_RANGES, CACHE_CONTROL, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, ETAG,
            ORIGIN, RANGE,
        },
        HeaderMap, HeaderValue, Method, Response, StatusCode,
    },
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, State as TauriState};
use tauri_plugin_dialog::{DialogExt, FilePath};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::sync::oneshot;
use tokio_util::io::ReaderStream;
use zip::{write::SimpleFileOptions, ZipArchive, ZipWriter};

const ROOT_REGISTRY_VERSION: u8 = 1;
const EXPORT_ENVELOPE_VERSION: u8 = 1;
const EXPORT_ENVELOPE_MAGIC: &[u8; 4] = b"ICX5";
const MANAGED_ROOT_ID: &str = "project-media";
const PROJECT_MEDIA_SUBDIRECTORY: &str = "画布素材";
const RELOCATE_MAX_DEPTH: usize = 8;
const RELOCATE_MAX_ENTRIES: usize = 50_000;
const MAX_IMPORT_PATHS: usize = 64;
const TEMPORARY_SYSTEM_PREFIXES: [&str; 4] =
    ["/private/var/folders", "/var/folders", "/private/tmp", "/tmp"];
const TEMPORARY_HOME_SUBDIRECTORIES: [&str; 3] = ["Library", "Downloads", ".Trash"];
const DURABLE_HOME_LIBRARY_SUBDIRECTORIES: [&str; 2] =
    ["Library/Mobile Documents", "Library/CloudStorage"];
const SCREENSHOT_NAME_PREFIXES: [&str; 8] = [
    "截屏",
    "屏幕快照",
    "屏幕录制",
    "screenshot",
    "screen shot",
    "screen recording",
    "cleanshot",
    "simulator screenshot",
];
const MAX_LOCAL_MEDIA_BYTES: u64 = 1024 * 1024 * 1024 * 1024;
const MAX_ARCHIVE_ENTRIES: usize = 20_000;
const MAX_ARCHIVE_UNCOMPRESSED_BYTES: u64 = 2 * 1024 * 1024 * 1024 * 1024;
const MAX_ARCHIVE_MANIFEST_BYTES: u64 = 64 * 1024 * 1024;
const MAX_RANGE_HEADER_BYTES: usize = 128;
const MAX_REQUEST_EVIDENCE: usize = 512;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LocalMediaReference {
    pub asset_id: String,
    pub storage_key: String,
    pub root_id: String,
    pub relative_path: String,
    pub sha256: String,
    pub mime_type: String,
    pub bytes: u64,
    pub file_name: String,
    pub width: Option<u64>,
    pub height: Option<u64>,
    pub duration_ms: Option<u64>,
    pub mode: LocalMediaMode,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LocalMediaMode {
    Reference,
    ProjectCopy,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LocalMediaResolution {
    pub reference: LocalMediaReference,
    pub status: &'static str,
    pub playback_url: Option<String>,
    pub reason: Option<&'static str>,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LocalMediaImportAction {
    Referenced,
    Moved,
    Copied,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LocalMediaImportDestination {
    InPlace,
    ProjectDirectory,
    ManagedRoot,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LocalMediaImportOutcome {
    pub resolution: LocalMediaResolution,
    pub action: LocalMediaImportAction,
    pub destination: LocalMediaImportDestination,
    pub temporary_source: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LocalMediaRequestEvidence {
    pub asset_id: String,
    pub method: String,
    pub requested_range: Option<String>,
    pub status: u16,
    pub response_bytes: u64,
    pub recorded_at_ms: u128,
}

pub(crate) struct TaskMediaReferenceInput<'a> {
    pub root_id: &'a str,
    pub root: &'a Path,
    pub relative: &'a Path,
    pub sha256: &'a str,
    pub mime_type: &'a str,
    pub width: Option<u64>,
    pub height: Option<u64>,
    pub duration_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DesktopCanvasImportResult {
    pub source_version: u64,
    pub projects: Vec<Value>,
    pub imported_media: usize,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DesktopExportEnvelope {
    version: u8,
    local_files: Vec<DesktopExportLocalFile>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DesktopExportLocalFile {
    path: String,
    reference: LocalMediaReference,
}

#[derive(Debug, Deserialize, Serialize)]
struct RootRegistry {
    version: u8,
    roots: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    project_media_dirs: HashMap<String, String>,
}

#[derive(Clone)]
struct RegisteredAsset {
    reference: LocalMediaReference,
    path: PathBuf,
    modified_nanos: u128,
}

pub(crate) struct LocalMediaManager {
    app_data_directory: PathBuf,
    roots_path: PathBuf,
    managed_root: PathBuf,
    roots: Mutex<RootRegistry>,
    assets: RwLock<HashMap<String, RegisteredAsset>>,
    credential: String,
    port: u16,
    web_port: u16,
    evidence: Mutex<Vec<LocalMediaRequestEvidence>>,
    shutdown: Mutex<Option<oneshot::Sender<()>>>,
}

impl LocalMediaManager {
    pub(crate) fn start(
        app_data_directory: &Path,
        port: u16,
        web_port: u16,
    ) -> Result<Arc<Self>, String> {
        let managed_root = app_data_directory.join("project-media");
        std::fs::create_dir_all(managed_root.join("owned"))
            .map_err(|error| format!("cannot create the managed media root: {error}"))?;
        let roots_path = app_data_directory.join("local-media-roots.json");
        let mut roots = load_root_registry(&roots_path)?;
        roots.roots.insert(
            MANAGED_ROOT_ID.to_owned(),
            path_to_private_string(&managed_root)?,
        );
        save_root_registry(&roots_path, &roots)?;
        let manager = Arc::new(Self {
            app_data_directory: app_data_directory.to_path_buf(),
            roots_path,
            managed_root,
            roots: Mutex::new(roots),
            assets: RwLock::new(HashMap::new()),
            credential: random_hex(32)?,
            port,
            web_port,
            evidence: Mutex::new(Vec::new()),
            shutdown: Mutex::new(None),
        });

        let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port))
            .map_err(|error| format!("local media port {port} is unavailable: {error}"))?;
        listener
            .set_nonblocking(true)
            .map_err(|error| format!("cannot configure local media listener: {error}"))?;
        let tokio_listener =
            tauri::async_runtime::block_on(
                async move { tokio::net::TcpListener::from_std(listener) },
            )
            .map_err(|error| format!("cannot activate local media listener: {error}"))?;
        let router = media_router(manager.clone());
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        *manager.shutdown.lock().unwrap() = Some(shutdown_tx);
        tauri::async_runtime::spawn(async move {
            let _ = axum::serve(tokio_listener, router)
                .with_graceful_shutdown(async move {
                    let _ = shutdown_rx.await;
                })
                .await;
        });
        Ok(manager)
    }

    pub(crate) fn shutdown(&self) {
        if let Some(shutdown) = self.shutdown.lock().unwrap().take() {
            let _ = shutdown.send(());
        }
    }

    pub(crate) fn request_evidence(&self) -> Vec<LocalMediaRequestEvidence> {
        self.evidence.lock().unwrap().clone()
    }

    pub(crate) fn register_selected_path(
        &self,
        selected: &Path,
        mode: LocalMediaMode,
    ) -> Result<LocalMediaResolution, String> {
        let selected = validate_selected_file(selected)?;
        let source_metadata = selected
            .metadata()
            .map_err(|error| format!("cannot inspect selected media: {error}"))?;
        ensure_media_size(source_metadata.len())?;
        let sha256 = sha256_file(&selected)?;
        let mime_type = mime_type_for_path(&selected)?;
        let probe = probe_media(&selected).unwrap_or_default();
        let file_name = selected
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| "the selected media name is not valid UTF-8".to_owned())?
            .to_owned();

        let (root_id, relative_path, path) = match mode {
            LocalMediaMode::Reference => {
                let root = selected
                    .parent()
                    .ok_or_else(|| "the selected media has no parent directory".to_owned())?
                    .to_path_buf();
                let root_id = self.register_user_root(&root)?;
                (root_id, file_name.clone(), selected)
            }
            LocalMediaMode::ProjectCopy => {
                let extension = selected
                    .extension()
                    .and_then(|value| value.to_str())
                    .unwrap_or("bin")
                    .to_ascii_lowercase();
                let asset_id = content_asset_id(&sha256);
                let relative_path = format!("owned/{asset_id}.{extension}");
                let target = self.managed_root.join(&relative_path);
                copy_verified_file(&selected, &target, &sha256)?;
                (MANAGED_ROOT_ID.to_owned(), relative_path, target)
            }
        };
        let asset_id = match mode {
            LocalMediaMode::Reference => reference_asset_id(&root_id, &relative_path, &sha256),
            LocalMediaMode::ProjectCopy => content_asset_id(&sha256),
        };
        let reference = LocalMediaReference {
            storage_key: format!("local-ref:{asset_id}"),
            asset_id,
            root_id,
            relative_path,
            sha256,
            mime_type,
            bytes: source_metadata.len(),
            file_name,
            width: probe.width,
            height: probe.height,
            duration_ms: probe.duration_ms,
            mode,
        };
        self.register_verified_reference(reference, path)
    }

    pub(crate) fn resolve_reference(&self, reference: LocalMediaReference) -> LocalMediaResolution {
        self.resolve_reference_for_project(None, reference)
    }

    /// Resolves a reference; when the file is no longer at its recorded path, looks for the
    /// same content (size + SHA-256) inside the project media directory and the reference root,
    /// and relinks it in place so moved or renamed files come back without a manual relink.
    pub(crate) fn resolve_reference_for_project(
        &self,
        project_id: Option<&str>,
        reference: LocalMediaReference,
    ) -> LocalMediaResolution {
        match self.resolve_and_verify(&reference) {
            Ok(path) => match self.register_verified_reference(reference.clone(), path) {
                Ok(result) => result,
                Err(_) => missing_resolution(reference, "unavailable"),
            },
            Err(LocalMediaResolveError::Missing) => {
                match self.relocate_missing_reference(project_id, &reference) {
                    Some(relocated) => relocated,
                    None => missing_resolution(reference, "missing"),
                }
            }
            Err(LocalMediaResolveError::DigestMismatch) => {
                match self.relocate_missing_reference(project_id, &reference) {
                    Some(relocated) => relocated,
                    None => missing_resolution(reference, "digest_mismatch"),
                }
            }
            Err(LocalMediaResolveError::Denied) => missing_resolution(reference, "denied"),
        }
    }

    fn relocate_missing_reference(
        &self,
        project_id: Option<&str>,
        reference: &LocalMediaReference,
    ) -> Option<LocalMediaResolution> {
        validate_reference_shape(reference).ok()?;
        let mut candidates: Vec<PathBuf> = Vec::new();
        if let Some(directory) = project_id.and_then(|id| self.project_media_directory(id)) {
            candidates.push(directory);
        }
        {
            let registry = self.roots.lock().unwrap();
            if let Some(root) = registry.roots.get(&reference.root_id) {
                candidates.push(PathBuf::from(root));
            }
        }
        let extension = Path::new(&reference.relative_path)
            .extension()
            .or_else(|| Path::new(&reference.file_name).extension())
            .and_then(|value| value.to_str())
            .map(|value| value.to_ascii_lowercase())?;
        let mut visited = 0usize;
        let mut searched: Vec<PathBuf> = Vec::new();
        for directory in candidates {
            let Ok(directory) = directory.canonicalize() else {
                continue;
            };
            if !directory.is_dir() || searched.iter().any(|done| directory.starts_with(done)) {
                continue;
            }
            if let Some(found) = find_file_by_digest(
                &directory,
                reference.bytes,
                &reference.sha256,
                &extension,
                RELOCATE_MAX_DEPTH,
                &mut visited,
            ) {
                let relinked = self
                    .relinked_reference(reference.clone(), &found, &reference.sha256)
                    .ok()?;
                return self.register_verified_reference(relinked, found).ok();
            }
            searched.push(directory);
        }
        None
    }

    pub(crate) fn project_media_directory(&self, project_id: &str) -> Option<PathBuf> {
        validate_project_id(project_id).ok()?;
        let registry = self.roots.lock().unwrap();
        registry
            .project_media_dirs
            .get(project_id)
            .map(PathBuf::from)
            .filter(|path| path.is_dir())
    }

    pub(crate) fn set_project_media_directory(
        &self,
        project_id: &str,
        directory: &Path,
    ) -> Result<String, String> {
        validate_project_id(project_id)?;
        let canonical = directory
            .canonicalize()
            .map_err(|error| format!("cannot resolve the project media directory: {error}"))?;
        if !canonical.is_dir() {
            return Err("the project media directory must be a directory".to_owned());
        }
        let app_data = self
            .app_data_directory
            .canonicalize()
            .unwrap_or_else(|_| self.app_data_directory.clone());
        if canonical.starts_with(&app_data) {
            return Err(
                "the project media directory cannot live inside the application data directory"
                    .to_owned(),
            );
        }
        let value = path_to_private_string(&canonical)?;
        let mut registry = self.roots.lock().unwrap();
        registry
            .project_media_dirs
            .insert(project_id.to_owned(), value.clone());
        save_root_registry(&self.roots_path, &registry)?;
        Ok(value)
    }

    /// Imports one local file with the desktop collection policy:
    /// - `ProjectCopy` keeps the explicit content-addressed copy inside the managed root;
    /// - durable files (project folders, user libraries) are referenced in place;
    /// - temporary sources (system temp, other apps' caches, Downloads, Trash, screenshots) are
    ///   moved into `<project media directory>/画布素材/` keeping their file name, or moved into
    ///   the managed root when the project has no media directory yet.
    pub(crate) fn import_path(
        &self,
        project_id: Option<&str>,
        path: &Path,
        requested: LocalMediaMode,
    ) -> Result<LocalMediaImportOutcome, String> {
        let selected = validate_selected_file(path)?;
        let temporary = self.is_temporary_source(&selected);
        self.import_path_with_policy(project_id, &selected, requested, temporary)
    }

    fn import_path_with_policy(
        &self,
        project_id: Option<&str>,
        path: &Path,
        requested: LocalMediaMode,
        temporary: bool,
    ) -> Result<LocalMediaImportOutcome, String> {
        let selected = validate_selected_file(path)?;
        if requested == LocalMediaMode::ProjectCopy {
            let resolution = self.register_selected_path(&selected, LocalMediaMode::ProjectCopy)?;
            return Ok(LocalMediaImportOutcome {
                resolution,
                action: LocalMediaImportAction::Copied,
                destination: LocalMediaImportDestination::ManagedRoot,
                temporary_source: temporary,
            });
        }
        if !temporary {
            let resolution = self.register_selected_path(&selected, LocalMediaMode::Reference)?;
            return Ok(LocalMediaImportOutcome {
                resolution,
                action: LocalMediaImportAction::Referenced,
                destination: LocalMediaImportDestination::InPlace,
                temporary_source: false,
            });
        }
        match project_id.and_then(|id| self.project_media_directory(id)) {
            Some(directory) => {
                let (target, moved) = collect_into_project_directory(&directory, &selected)?;
                let resolution = self.register_selected_path(&target, LocalMediaMode::Reference)?;
                Ok(LocalMediaImportOutcome {
                    resolution,
                    action: if moved {
                        LocalMediaImportAction::Moved
                    } else {
                        LocalMediaImportAction::Referenced
                    },
                    destination: LocalMediaImportDestination::ProjectDirectory,
                    temporary_source: true,
                })
            }
            None => {
                let resolution =
                    self.register_selected_path(&selected, LocalMediaMode::ProjectCopy)?;
                let removed = std::fs::remove_file(&selected).is_ok();
                Ok(LocalMediaImportOutcome {
                    resolution,
                    action: if removed {
                        LocalMediaImportAction::Moved
                    } else {
                        LocalMediaImportAction::Copied
                    },
                    destination: LocalMediaImportDestination::ManagedRoot,
                    temporary_source: true,
                })
            }
        }
    }

    fn is_temporary_source(&self, path: &Path) -> bool {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        is_temporary_source_with_home(path, &self.app_data_directory, home.as_deref())
    }

    fn relinked_reference(
        &self,
        reference: LocalMediaReference,
        selected: &Path,
        sha256: &str,
    ) -> Result<LocalMediaReference, String> {
        let metadata = selected
            .metadata()
            .map_err(|error| format!("cannot inspect relinked media: {error}"))?;
        ensure_media_size(metadata.len())?;
        let parent = selected
            .parent()
            .ok_or_else(|| "the relinked media has no parent directory".to_owned())?;
        let root_id = self.register_user_root(parent)?;
        let relative_path = selected
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| "the relinked media name is not valid UTF-8".to_owned())?
            .to_owned();
        let mut next = reference;
        next.root_id = root_id;
        next.relative_path = relative_path.clone();
        next.asset_id = reference_asset_id(&next.root_id, &next.relative_path, sha256);
        next.storage_key = format!("local-ref:{}", next.asset_id);
        next.file_name = relative_path;
        next.bytes = metadata.len();
        next.mode = LocalMediaMode::Reference;
        Ok(next)
    }

    pub(crate) fn relink_reference(
        &self,
        reference: LocalMediaReference,
        selected: &Path,
    ) -> Result<LocalMediaResolution, String> {
        validate_reference_shape(&reference)?;
        let selected = validate_selected_file(selected)?;
        let metadata = selected
            .metadata()
            .map_err(|error| format!("cannot inspect relinked media: {error}"))?;
        ensure_media_size(metadata.len())?;
        let sha256 = sha256_file(&selected)?;
        if sha256 != reference.sha256 {
            return Err(
                "重新定位的文件与原素材 SHA-256 不一致；请使用“替换视频”明确更换素材".to_owned(),
            );
        }
        let next = self.relinked_reference(reference, &selected, &sha256)?;
        self.register_verified_reference(next, selected)
    }

    pub(crate) fn reference_for_task_media(
        &self,
        input: TaskMediaReferenceInput<'_>,
    ) -> Result<LocalMediaResolution, String> {
        let relative_path = relative_path_string(input.relative)?;
        let path = validate_path_under_root(input.root, input.relative)?;
        let metadata = path
            .metadata()
            .map_err(|_| "the verified desktop task media is missing".to_owned())?;
        ensure_media_size(metadata.len())?;
        if sha256_file(&path)? != input.sha256 {
            return Err("the verified desktop task media digest changed".to_owned());
        }
        self.register_fixed_root(input.root_id, input.root)?;
        let asset_id = reference_asset_id(input.root_id, &relative_path, input.sha256);
        let reference = LocalMediaReference {
            storage_key: format!("local-ref:{asset_id}"),
            asset_id,
            root_id: input.root_id.to_owned(),
            relative_path,
            sha256: input.sha256.to_owned(),
            mime_type: input.mime_type.to_owned(),
            bytes: metadata.len(),
            file_name: path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("media")
                .to_owned(),
            width: input.width,
            height: input.height,
            duration_ms: input.duration_ms,
            mode: LocalMediaMode::ProjectCopy,
        };
        self.register_verified_reference(reference, path)
    }

    pub(crate) fn import_archive(
        &self,
        archive_path: &Path,
    ) -> Result<DesktopCanvasImportResult, String> {
        let file = File::open(archive_path)
            .map_err(|error| format!("cannot open canvas archive: {error}"))?;
        let mut archive = ZipArchive::new(BufReader::new(file))
            .map_err(|error| format!("invalid canvas ZIP archive: {error}"))?;
        validate_archive_limits(&mut archive)?;
        let manifest: Value = {
            let mut entry = archive
                .by_name("projects.json")
                .map_err(|_| "canvas archive is missing projects.json".to_owned())?;
            if entry.size() == 0 || entry.size() > MAX_ARCHIVE_MANIFEST_BYTES {
                return Err("canvas archive manifest crossed the fixed size boundary".to_owned());
            }
            serde_json::from_reader(&mut entry)
                .map_err(|error| format!("invalid canvas manifest: {error}"))?
        };
        if manifest.get("app").and_then(Value::as_str) != Some("infinite-canvas") {
            return Err("canvas archive app identifier is invalid".to_owned());
        }
        let source_version = manifest
            .get("version")
            .and_then(Value::as_u64)
            .ok_or_else(|| "canvas archive version is missing".to_owned())?;
        if !(3..=5).contains(&source_version) {
            return Err(format!(
                "canvas archive version {source_version} is unsupported"
            ));
        }
        let items = manifest
            .get("projects")
            .and_then(Value::as_array)
            .ok_or_else(|| "canvas archive projects are invalid".to_owned())?;
        let mut projects = Vec::with_capacity(items.len());
        let mut imported_media = 0usize;
        for item in items {
            let mut project = item
                .get("project")
                .cloned()
                .ok_or_else(|| "canvas archive project entry is invalid".to_owned())?;
            let files = item
                .get("files")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            for file_manifest in files {
                let old_storage_key = file_manifest
                    .get("storageKey")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "canvas archive storage key is invalid".to_owned())?;
                let embedded = file_manifest
                    .get("embedded")
                    .and_then(Value::as_bool)
                    .unwrap_or(true);
                let path = file_manifest
                    .get("path")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if !embedded || path.is_empty() {
                    continue;
                }
                validate_archive_relative_path(path)?;
                let declared_mime_type = file_manifest
                    .get("mimeType")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let inferred_mime_type = mime_type_for_path(Path::new(path))?;
                if !declared_mime_type.is_empty() && declared_mime_type != inferred_mime_type {
                    return Err(format!(
                        "canvas archive media MIME type does not match its file name: {path}"
                    ));
                }
                let mut entry = archive
                    .by_name(path)
                    .map_err(|_| format!("canvas archive media is missing: {path}"))?;
                if entry.size() == 0 || entry.size() > MAX_LOCAL_MEDIA_BYTES {
                    return Err("canvas archive media crossed the size boundary".to_owned());
                }
                if let Some(expected) = file_manifest.get("bytes").and_then(Value::as_u64) {
                    if expected != entry.size() {
                        return Err(format!("canvas archive media size does not match: {path}"));
                    }
                }
                let reference =
                    self.import_embedded_media(&mut entry, path, &inferred_mime_type)?;
                if let Some(expected_sha256) = file_manifest
                    .pointer("/reference/sha256")
                    .and_then(Value::as_str)
                {
                    validate_sha256(expected_sha256)?;
                    if reference.sha256 != expected_sha256 {
                        return Err(format!(
                            "canvas archive media digest does not match: {path}"
                        ));
                    }
                }
                rewrite_storage_reference(&mut project, old_storage_key, &reference);
                imported_media += 1;
            }
            projects.push(project);
        }
        Ok(DesktopCanvasImportResult {
            source_version,
            projects,
            imported_media,
        })
    }

    pub(crate) fn export_archive(
        &self,
        target: &Path,
        base_zip: &[u8],
        envelope: DesktopExportEnvelope,
    ) -> Result<u64, String> {
        if envelope.version != EXPORT_ENVELOPE_VERSION {
            return Err("desktop export envelope version is unsupported".to_owned());
        }
        if envelope.local_files.len() > MAX_ARCHIVE_ENTRIES {
            return Err("desktop export local media count crossed the fixed boundary".to_owned());
        }
        let local_media_bytes = envelope.local_files.iter().try_fold(0u64, |total, local| {
            validate_reference_shape(&local.reference)?;
            total
                .checked_add(local.reference.bytes)
                .ok_or_else(|| "desktop export local media size overflow".to_owned())
        })?;
        if local_media_bytes > MAX_ARCHIVE_UNCOMPRESSED_BYTES {
            return Err("desktop export local media crossed the fixed size boundary".to_owned());
        }
        validate_zip_target(target)?;
        let staging = unique_staging_path(target)?;
        let result = (|| {
            let mut source = ZipArchive::new(std::io::Cursor::new(base_zip))
                .map_err(|error| format!("invalid base canvas ZIP: {error}"))?;
            validate_archive_limits(&mut source)?;
            let staging_file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&staging)
                .map_err(|error| format!("cannot create export staging file: {error}"))?;
            let mut writer = ZipWriter::new(staging_file);
            let options =
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
            let mut written_paths = HashSet::new();
            for index in 0..source.len() {
                let mut entry = source
                    .by_index(index)
                    .map_err(|error| format!("cannot read base ZIP entry: {error}"))?;
                if entry.is_dir() {
                    continue;
                }
                let name = entry.name().to_owned();
                validate_archive_relative_path(&name)?;
                if !written_paths.insert(name.clone()) {
                    return Err("base canvas ZIP contains duplicate paths".to_owned());
                }
                writer
                    .start_file(name, options)
                    .map_err(|error| format!("cannot write base ZIP entry: {error}"))?;
                std::io::copy(&mut entry, &mut writer)
                    .map_err(|error| format!("cannot copy base ZIP entry: {error}"))?;
            }
            for local in envelope.local_files {
                validate_embedded_media_path(&local.path)?;
                if !written_paths.insert(local.path.clone()) {
                    return Err("desktop export tried to overwrite a base ZIP path".to_owned());
                }
                let path = self
                    .resolve_and_verify(&local.reference)
                    .map_err(|error| format!("cannot embed local media: {error:?}"))?;
                writer
                    .start_file(local.path, options)
                    .map_err(|error| format!("cannot create local media ZIP entry: {error}"))?;
                let mut input = File::open(path)
                    .map_err(|error| format!("cannot open local media for export: {error}"))?;
                std::io::copy(&mut input, &mut writer)
                    .map_err(|error| format!("cannot stream local media into export: {error}"))?;
            }
            let output = writer
                .finish()
                .map_err(|error| format!("cannot finish canvas ZIP: {error}"))?;
            output
                .sync_all()
                .map_err(|error| format!("cannot sync canvas ZIP: {error}"))?;
            std::fs::hard_link(&staging, target).map_err(|error| {
                format!("cannot publish canvas ZIP without overwriting: {error}")
            })?;
            std::fs::metadata(target)
                .map(|metadata| metadata.len())
                .map_err(|error| format!("cannot inspect exported canvas ZIP: {error}"))
        })();
        let _ = std::fs::remove_file(staging);
        result
    }

    fn import_embedded_media(
        &self,
        input: &mut impl Read,
        archive_path: &str,
        mime_type: &str,
    ) -> Result<LocalMediaReference, String> {
        let extension = Path::new(archive_path)
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_else(|| extension_for_mime(mime_type))
            .to_ascii_lowercase();
        let staging = self.managed_root.join("owned").join(format!(
            ".import-{}-{}.part",
            std::process::id(),
            random_hex(8)?
        ));
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staging)
            .map_err(|error| format!("cannot create imported media staging file: {error}"))?;
        let mut digest = Sha256::new();
        let mut bytes = 0u64;
        let mut buffer = vec![0u8; 1024 * 1024];
        let copy_result = (|| {
            loop {
                let read = input
                    .read(&mut buffer)
                    .map_err(|error| format!("cannot read imported media: {error}"))?;
                if read == 0 {
                    break;
                }
                bytes = bytes
                    .checked_add(read as u64)
                    .ok_or_else(|| "imported media size overflow".to_owned())?;
                ensure_media_size(bytes)?;
                digest.update(&buffer[..read]);
                output
                    .write_all(&buffer[..read])
                    .map_err(|error| format!("cannot write imported media: {error}"))?;
            }
            if bytes == 0 {
                return Err("imported media is empty".to_owned());
            }
            output
                .sync_all()
                .map_err(|error| format!("cannot sync imported media: {error}"))?;
            Ok(())
        })();
        drop(output);
        if let Err(error) = copy_result {
            let _ = std::fs::remove_file(&staging);
            return Err(error);
        }
        let sha256 = format!("{:x}", digest.finalize());
        let asset_id = content_asset_id(&sha256);
        let relative_path = format!("owned/{asset_id}.{extension}");
        let target = self.managed_root.join(&relative_path);
        if target.exists() {
            if sha256_file(&target)? != sha256 {
                let _ = std::fs::remove_file(&staging);
                return Err("managed media hash collision".to_owned());
            }
            let _ = std::fs::remove_file(&staging);
        } else {
            std::fs::rename(&staging, &target)
                .map_err(|error| format!("cannot publish imported media: {error}"))?;
        }
        let probe = probe_media(&target).unwrap_or_default();
        let reference = LocalMediaReference {
            asset_id: asset_id.clone(),
            storage_key: format!("local-ref:{asset_id}"),
            root_id: MANAGED_ROOT_ID.to_owned(),
            relative_path,
            sha256,
            mime_type: mime_type.to_owned(),
            bytes,
            file_name: Path::new(archive_path)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("media")
                .to_owned(),
            width: probe.width,
            height: probe.height,
            duration_ms: probe.duration_ms,
            mode: LocalMediaMode::ProjectCopy,
        };
        let resolution = self.register_verified_reference(reference.clone(), target)?;
        if resolution.status != "available" {
            return Err("imported media could not be registered".to_owned());
        }
        Ok(reference)
    }

    fn register_user_root(&self, root: &Path) -> Result<String, String> {
        let canonical = root
            .canonicalize()
            .map_err(|error| format!("cannot authorize selected media root: {error}"))?;
        let value = path_to_private_string(&canonical)?;
        let mut registry = self.roots.lock().unwrap();
        if let Some((root_id, _)) = registry.roots.iter().find(|(_, path)| *path == &value) {
            return Ok(root_id.clone());
        }
        let root_id = format!("root-{}", random_hex(16)?);
        registry.roots.insert(root_id.clone(), value);
        save_root_registry(&self.roots_path, &registry)?;
        Ok(root_id)
    }

    fn register_fixed_root(&self, root_id: &str, root: &Path) -> Result<(), String> {
        validate_root_id(root_id)?;
        let root = root
            .canonicalize()
            .map_err(|error| format!("cannot register fixed media root: {error}"))?;
        let value = path_to_private_string(&root)?;
        let mut registry = self.roots.lock().unwrap();
        if let Some(existing) = registry.roots.get(root_id) {
            if existing != &value {
                return Err("fixed media root identifier conflict".to_owned());
            }
            return Ok(());
        }
        registry.roots.insert(root_id.to_owned(), value);
        save_root_registry(&self.roots_path, &registry)
    }

    fn resolve_and_verify(
        &self,
        reference: &LocalMediaReference,
    ) -> Result<PathBuf, LocalMediaResolveError> {
        validate_reference_shape(reference).map_err(|_| LocalMediaResolveError::Denied)?;
        if let Some(cached) = self
            .assets
            .read()
            .unwrap()
            .get(&reference.asset_id)
            .cloned()
        {
            if cached.reference == *reference {
                let link_metadata = std::fs::symlink_metadata(&cached.path)
                    .map_err(|_| LocalMediaResolveError::Missing)?;
                if link_metadata.file_type().is_symlink() {
                    return Err(LocalMediaResolveError::Denied);
                }
                let metadata =
                    std::fs::metadata(&cached.path).map_err(|_| LocalMediaResolveError::Missing)?;
                let modified_nanos = modified_nanos(&metadata).unwrap_or_default();
                if metadata.is_file()
                    && metadata.len() == reference.bytes
                    && modified_nanos == cached.modified_nanos
                {
                    return Ok(cached.path);
                }
            }
        }
        let root = {
            let registry = self.roots.lock().unwrap();
            registry
                .roots
                .get(&reference.root_id)
                .map(PathBuf::from)
                .ok_or(LocalMediaResolveError::Missing)?
        };
        let relative = Path::new(&reference.relative_path);
        if !root.exists() || std::fs::symlink_metadata(root.join(relative)).is_err() {
            return Err(LocalMediaResolveError::Missing);
        }
        let path = validate_path_under_root(&root, relative)
            .map_err(|_| LocalMediaResolveError::Denied)?;
        let metadata = path
            .metadata()
            .map_err(|_| LocalMediaResolveError::Missing)?;
        if !metadata.is_file() || metadata.len() != reference.bytes {
            return Err(LocalMediaResolveError::Missing);
        }
        ensure_media_size(metadata.len()).map_err(|_| LocalMediaResolveError::Denied)?;
        let sha256 = sha256_file(&path).map_err(|_| LocalMediaResolveError::Missing)?;
        if sha256 != reference.sha256 {
            return Err(LocalMediaResolveError::DigestMismatch);
        }
        Ok(path)
    }

    fn register_verified_reference(
        &self,
        reference: LocalMediaReference,
        path: PathBuf,
    ) -> Result<LocalMediaResolution, String> {
        validate_reference_shape(&reference)?;
        let link_metadata = std::fs::symlink_metadata(&path)
            .map_err(|error| format!("cannot inspect verified local media: {error}"))?;
        if link_metadata.file_type().is_symlink() || !link_metadata.is_file() {
            return Err("verified local media must be a regular non-symlink file".to_owned());
        }
        let metadata = path
            .metadata()
            .map_err(|error| format!("cannot inspect verified local media: {error}"))?;
        let modified_nanos = modified_nanos(&metadata)?;
        self.assets.write().unwrap().insert(
            reference.asset_id.clone(),
            RegisteredAsset {
                reference: reference.clone(),
                path,
                modified_nanos,
            },
        );
        Ok(LocalMediaResolution {
            playback_url: Some(self.playback_url(&reference.asset_id)),
            reference,
            status: "available",
            reason: None,
        })
    }

    #[cfg(test)]
    pub(crate) fn offline_for_tests(app_data_directory: &Path) -> Result<Arc<Self>, String> {
        let managed_root = app_data_directory.join("project-media");
        std::fs::create_dir_all(managed_root.join("owned"))
            .map_err(|error| format!("cannot create the managed media root: {error}"))?;
        let roots_path = app_data_directory.join("local-media-roots.json");
        let mut roots = load_root_registry(&roots_path)?;
        roots.roots.insert(
            MANAGED_ROOT_ID.to_owned(),
            path_to_private_string(&managed_root)?,
        );
        save_root_registry(&roots_path, &roots)?;
        Ok(Arc::new(Self {
            app_data_directory: app_data_directory.to_path_buf(),
            roots_path,
            managed_root,
            roots: Mutex::new(roots),
            assets: RwLock::new(HashMap::new()),
            credential: "offline-test-credential".to_owned(),
            port: 0,
            web_port: 0,
            evidence: Mutex::new(Vec::new()),
            shutdown: Mutex::new(None),
        }))
    }

    pub(crate) fn app_data_directory(&self) -> &Path {
        &self.app_data_directory
    }

    pub(crate) fn read_verified_media(
        &self,
        reference: &LocalMediaReference,
        max_bytes: u64,
    ) -> Result<Vec<u8>, String> {
        if reference.bytes > max_bytes {
            return Err("the referenced media crossed the fixed read boundary".to_owned());
        }
        let path = self
            .resolve_and_verify(reference)
            .map_err(|error| format!("the referenced media is unavailable: {error:?}"))?;
        std::fs::read(path).map_err(|error| format!("cannot read the referenced media: {error}"))
    }

    fn playback_url(&self, asset_id: &str) -> String {
        format!(
            "http://127.0.0.1:{}/v1/media/{}?token={}",
            self.port, asset_id, self.credential
        )
    }

    fn record_request(&self, evidence: LocalMediaRequestEvidence) {
        let mut records = self.evidence.lock().unwrap();
        records.push(evidence);
        if records.len() > MAX_REQUEST_EVIDENCE {
            let excess = records.len() - MAX_REQUEST_EVIDENCE;
            records.drain(0..excess);
        }
    }
}

#[derive(Debug)]
enum LocalMediaResolveError {
    Missing,
    DigestMismatch,
    Denied,
}

#[derive(Default)]
pub(crate) struct MediaProbeSummary {
    pub(crate) width: Option<u64>,
    pub(crate) height: Option<u64>,
    pub(crate) duration_ms: Option<u64>,
}

#[derive(Deserialize)]
struct StreamQuery {
    token: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ByteRange {
    start: u64,
    end: u64,
}

fn media_router(manager: Arc<LocalMediaManager>) -> Router {
    Router::new()
        .route("/v1/media/:asset_id", get(stream_media).head(stream_media))
        .with_state(manager)
}

async fn stream_media(
    State(manager): State<Arc<LocalMediaManager>>,
    AxumPath(asset_id): AxumPath<String>,
    Query(query): Query<StreamQuery>,
    method: Method,
    headers: HeaderMap,
) -> Response<Body> {
    let requested_range = headers
        .get(RANGE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    if !constant_time_eq(
        query.token.as_deref().unwrap_or_default().as_bytes(),
        manager.credential.as_bytes(),
    ) {
        return media_error(
            &manager,
            &asset_id,
            method,
            requested_range,
            StatusCode::UNAUTHORIZED,
        );
    }
    if let Some(origin) = headers.get(ORIGIN).and_then(|value| value.to_str().ok()) {
        let expected = format!("http://127.0.0.1:{}", manager.web_port);
        if origin != expected {
            return media_error(
                &manager,
                &asset_id,
                method,
                requested_range,
                StatusCode::FORBIDDEN,
            );
        }
    }
    if !valid_asset_id(&asset_id) {
        return media_error(
            &manager,
            &asset_id,
            method,
            requested_range,
            StatusCode::NOT_FOUND,
        );
    }
    let Some(asset) = manager.assets.read().unwrap().get(&asset_id).cloned() else {
        return media_error(
            &manager,
            &asset_id,
            method,
            requested_range,
            StatusCode::NOT_FOUND,
        );
    };
    let path = match manager.resolve_and_verify(&asset.reference) {
        Ok(path) => path,
        Err(_) => {
            return media_error(
                &manager,
                &asset_id,
                method,
                requested_range,
                StatusCode::NOT_FOUND,
            )
        }
    };
    let total = asset.reference.bytes;
    let range = match requested_range.as_deref() {
        Some(value) => match parse_single_range(value, total) {
            Ok(range) => Some(range),
            Err(_) => {
                let mut response = media_error(
                    &manager,
                    &asset_id,
                    method,
                    requested_range,
                    StatusCode::RANGE_NOT_SATISFIABLE,
                );
                response.headers_mut().insert(
                    CONTENT_RANGE,
                    HeaderValue::from_str(&format!("bytes */{total}")).unwrap(),
                );
                return response;
            }
        },
        None => None,
    };
    let (status, start, end) = match range {
        Some(range) => (StatusCode::PARTIAL_CONTENT, range.start, range.end),
        None => (StatusCode::OK, 0, total - 1),
    };
    let response_bytes = end - start + 1;
    let mut builder = Response::builder()
        .status(status)
        .header(ACCEPT_RANGES, "bytes")
        .header(CACHE_CONTROL, "private, no-store")
        .header(CONTENT_TYPE, asset.reference.mime_type.as_str())
        .header(CONTENT_LENGTH, response_bytes.to_string())
        .header(ETAG, format!("\"sha256:{}\"", asset.reference.sha256));
    if status == StatusCode::PARTIAL_CONTENT {
        builder = builder.header(CONTENT_RANGE, format!("bytes {start}-{end}/{total}"));
    }
    let body = if method == Method::HEAD {
        Body::empty()
    } else {
        match tokio::fs::File::open(path).await {
            Ok(mut file) => {
                if file.seek(std::io::SeekFrom::Start(start)).await.is_err() {
                    return media_error(
                        &manager,
                        &asset_id,
                        method,
                        requested_range,
                        StatusCode::INTERNAL_SERVER_ERROR,
                    );
                }
                let stream = ReaderStream::new(file.take(response_bytes));
                Body::from_stream(stream)
            }
            Err(_) => {
                return media_error(
                    &manager,
                    &asset_id,
                    method,
                    requested_range,
                    StatusCode::NOT_FOUND,
                )
            }
        }
    };
    manager.record_request(LocalMediaRequestEvidence {
        asset_id,
        method: method.to_string(),
        requested_range,
        status: status.as_u16(),
        response_bytes,
        recorded_at_ms: now_millis(),
    });
    builder
        .body(body)
        .unwrap_or_else(|_| Body::empty().into_response())
}

trait IntoBodyResponse {
    fn into_response(self) -> Response<Body>;
}

impl IntoBodyResponse for Body {
    fn into_response(self) -> Response<Body> {
        Response::new(self)
    }
}

fn media_error(
    manager: &LocalMediaManager,
    asset_id: &str,
    method: Method,
    requested_range: Option<String>,
    status: StatusCode,
) -> Response<Body> {
    manager.record_request(LocalMediaRequestEvidence {
        asset_id: asset_id.to_owned(),
        method: method.to_string(),
        requested_range,
        status: status.as_u16(),
        response_bytes: 0,
        recorded_at_ms: now_millis(),
    });
    Response::builder()
        .status(status)
        .header(CACHE_CONTROL, "no-store")
        .body(Body::empty())
        .unwrap()
}

fn parse_single_range(value: &str, total: u64) -> Result<ByteRange, ()> {
    if total == 0 || value.len() > MAX_RANGE_HEADER_BYTES || !value.starts_with("bytes=") {
        return Err(());
    }
    let spec = &value[6..];
    if spec.is_empty() || spec.contains(',') || spec.chars().any(char::is_whitespace) {
        return Err(());
    }
    let (start, end) = spec.split_once('-').ok_or(())?;
    if start.is_empty() {
        let suffix = end.parse::<u64>().map_err(|_| ())?;
        if suffix == 0 {
            return Err(());
        }
        let length = suffix.min(total);
        return Ok(ByteRange {
            start: total - length,
            end: total - 1,
        });
    }
    let start = start.parse::<u64>().map_err(|_| ())?;
    if start >= total {
        return Err(());
    }
    let end = if end.is_empty() {
        total - 1
    } else {
        end.parse::<u64>().map_err(|_| ())?.min(total - 1)
    };
    if end < start {
        return Err(());
    }
    Ok(ByteRange { start, end })
}

fn missing_resolution(
    reference: LocalMediaReference,
    reason: &'static str,
) -> LocalMediaResolution {
    LocalMediaResolution {
        reference,
        status: "missing",
        playback_url: None,
        reason: Some(reason),
    }
}

fn validate_reference_shape(reference: &LocalMediaReference) -> Result<(), String> {
    if !valid_asset_id(&reference.asset_id)
        || reference.storage_key != format!("local-ref:{}", reference.asset_id)
    {
        return Err("local media asset identifier is invalid".to_owned());
    }
    validate_root_id(&reference.root_id)?;
    validate_relative_path(Path::new(&reference.relative_path))?;
    validate_sha256(&reference.sha256)?;
    let content_id = content_asset_id(&reference.sha256);
    let path_id = reference_asset_id(
        &reference.root_id,
        &reference.relative_path,
        &reference.sha256,
    );
    if reference.asset_id != content_id && reference.asset_id != path_id {
        return Err("local media asset identifier does not match its controlled source".to_owned());
    }
    if reference.bytes == 0 || reference.bytes > MAX_LOCAL_MEDIA_BYTES {
        return Err("local media size is outside the allowed range".to_owned());
    }
    if !supported_mime_type(&reference.mime_type) {
        return Err("local media MIME type is unsupported".to_owned());
    }
    Ok(())
}

fn validate_project_id(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 200
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err("canvas project identifier is invalid".to_owned());
    }
    Ok(())
}

/// A source is "temporary" when it lives somewhere the user does not curate (system temp,
/// other applications' private data and caches, Downloads, Trash) or when its name marks it as a
/// screenshot/recording that macOS dropped on the Desktop. iCloud Drive and cloud-storage mounts
/// under ~/Library are durable user folders and are never treated as temporary.
fn is_temporary_source_with_home(path: &Path, app_data_directory: &Path, home: Option<&Path>) -> bool {
    let app_data = app_data_directory
        .canonicalize()
        .unwrap_or_else(|_| app_data_directory.to_path_buf());
    if path.starts_with(&app_data) || path.starts_with(app_data_directory) {
        return false;
    }
    if TEMPORARY_SYSTEM_PREFIXES
        .iter()
        .any(|prefix| path.starts_with(prefix))
    {
        return true;
    }
    if let Some(home) = home {
        if DURABLE_HOME_LIBRARY_SUBDIRECTORIES
            .iter()
            .any(|sub| path.starts_with(home.join(sub)))
        {
            return false;
        }
        if TEMPORARY_HOME_SUBDIRECTORIES
            .iter()
            .any(|sub| path.starts_with(home.join(sub)))
        {
            return true;
        }
    }
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.to_lowercase())
        .unwrap_or_default();
    SCREENSHOT_NAME_PREFIXES
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

/// Moves a temporary source into `<directory>/画布素材/` keeping its file name. Returns the
/// final path and whether a move happened. Same content already present in that folder is reused
/// (and the redundant temporary source removed); a name clash with different content gets a
/// numeric suffix. Sources already inside the project directory stay where they are.
fn collect_into_project_directory(directory: &Path, source: &Path) -> Result<(PathBuf, bool), String> {
    let directory = directory
        .canonicalize()
        .map_err(|error| format!("cannot resolve the project media directory: {error}"))?;
    if source.starts_with(&directory) {
        return Ok((source.to_path_buf(), false));
    }
    let metadata = source
        .metadata()
        .map_err(|error| format!("cannot inspect the temporary media: {error}"))?;
    let sha256 = sha256_file(source)?;
    let folder = directory.join(PROJECT_MEDIA_SUBDIRECTORY);
    std::fs::create_dir_all(&folder)
        .map_err(|error| format!("cannot create the project media folder: {error}"))?;
    let extension = source
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .unwrap_or_default();
    let mut visited = 0usize;
    if let Some(existing) =
        find_file_by_digest(&folder, metadata.len(), &sha256, &extension, 1, &mut visited)
    {
        let _ = std::fs::remove_file(source);
        return Ok((existing, true));
    }
    let file_name = source
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "the temporary media name is not valid UTF-8".to_owned())?;
    let target = unique_collection_target(&folder, file_name)?;
    move_file_verified(source, &target, &sha256)?;
    Ok((target, true))
}

fn unique_collection_target(folder: &Path, file_name: &str) -> Result<PathBuf, String> {
    let candidate = folder.join(file_name);
    if !candidate.exists() {
        return Ok(candidate);
    }
    let path = Path::new(file_name);
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("media");
    let extension = path.extension().and_then(|value| value.to_str());
    for index in 2..10_000u32 {
        let name = match extension {
            Some(extension) => format!("{stem}-{index}.{extension}"),
            None => format!("{stem}-{index}"),
        };
        let candidate = folder.join(name);
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err("cannot find a free file name in the project media folder".to_owned())
}

fn move_file_verified(source: &Path, target: &Path, expected_sha256: &str) -> Result<(), String> {
    if target.exists() {
        return Err("project media target already exists".to_owned());
    }
    if std::fs::rename(source, target).is_ok() {
        return Ok(());
    }
    copy_verified_file(source, target, expected_sha256)?;
    std::fs::remove_file(source)
        .map_err(|error| format!("copied the media but cannot remove the temporary source: {error}"))
}

/// Bounded recursive search for a regular file with the given size, extension and SHA-256.
/// Hidden entries, symlinks and staging files are skipped; size and extension are checked before
/// hashing so large trees stay cheap.
fn find_file_by_digest(
    directory: &Path,
    bytes: u64,
    sha256: &str,
    extension: &str,
    max_depth: usize,
    visited: &mut usize,
) -> Option<PathBuf> {
    let entries = std::fs::read_dir(directory).ok()?;
    let mut subdirectories = Vec::new();
    for entry in entries.flatten() {
        *visited += 1;
        if *visited > RELOCATE_MAX_ENTRIES {
            return None;
        }
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            continue;
        }
        let path = entry.path();
        if file_type.is_dir() {
            if max_depth > 1 {
                subdirectories.push(path);
            }
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        let matches_extension = path
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| value.eq_ignore_ascii_case(extension))
            .unwrap_or(extension.is_empty());
        if !matches_extension {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if metadata.len() != bytes {
            continue;
        }
        if sha256_file(&path).ok().as_deref() == Some(sha256) {
            return path.canonicalize().ok();
        }
    }
    for subdirectory in subdirectories {
        if let Some(found) =
            find_file_by_digest(&subdirectory, bytes, sha256, extension, max_depth - 1, visited)
        {
            return Some(found);
        }
    }
    None
}

fn validate_root_id(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 80
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err("local media root identifier is invalid".to_owned());
    }
    Ok(())
}

fn valid_asset_id(value: &str) -> bool {
    value.starts_with("asset-")
        && value.len() <= 80
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn validate_sha256(value: &str) -> Result<(), String> {
    if value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Ok(());
    }
    Err("local media SHA-256 is invalid".to_owned())
}

fn validate_relative_path(path: &Path) -> Result<(), String> {
    let raw = path
        .to_str()
        .ok_or_else(|| "local media relative path is not valid UTF-8".to_owned())?;
    if raw.is_empty() || raw.len() > 1024 || raw.chars().any(char::is_control) || path.is_absolute()
    {
        return Err("local media relative path is invalid".to_owned());
    }
    for component in path.components() {
        if !matches!(component, Component::Normal(_)) {
            return Err("local media relative path crossed the approved root".to_owned());
        }
    }
    Ok(())
}

fn validate_path_under_root(root: &Path, relative: &Path) -> Result<PathBuf, String> {
    validate_relative_path(relative)?;
    let canonical_root = root
        .canonicalize()
        .map_err(|_| "the approved local media root is missing".to_owned())?;
    let mut cursor = canonical_root.clone();
    for component in relative.components() {
        let Component::Normal(component) = component else {
            return Err("local media path traversal was rejected".to_owned());
        };
        cursor.push(component);
        let metadata =
            std::fs::symlink_metadata(&cursor).map_err(|_| "local media is missing".to_owned())?;
        if metadata.file_type().is_symlink() {
            return Err("local media symbolic links are not allowed".to_owned());
        }
    }
    let canonical = cursor
        .canonicalize()
        .map_err(|_| "local media is missing".to_owned())?;
    if !canonical.starts_with(&canonical_root) {
        return Err("local media escaped the approved root".to_owned());
    }
    Ok(canonical)
}

fn validate_selected_file(path: &Path) -> Result<PathBuf, String> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|_| "the selected local media is missing".to_owned())?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("symbolic links and non-files cannot be selected as local media".to_owned());
    }
    ensure_media_size(metadata.len())?;
    mime_type_for_path(path)?;
    path.canonicalize()
        .map_err(|error| format!("cannot resolve selected local media: {error}"))
}

fn ensure_media_size(bytes: u64) -> Result<(), String> {
    if bytes == 0 || bytes > MAX_LOCAL_MEDIA_BYTES {
        return Err(format!(
            "local media size must be between 1 byte and {MAX_LOCAL_MEDIA_BYTES} bytes"
        ));
    }
    Ok(())
}

pub(crate) fn mime_type_for_path(path: &Path) -> Result<String, String> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let mime = match extension.as_str() {
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "m4a" => "audio/mp4",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => return Err("selected local media format is unsupported".to_owned()),
    };
    Ok(mime.to_owned())
}

fn supported_mime_type(value: &str) -> bool {
    matches!(
        value,
        "video/mp4"
            | "video/quicktime"
            | "video/webm"
            | "audio/mpeg"
            | "audio/wav"
            | "audio/mp4"
            | "image/png"
            | "image/jpeg"
            | "image/webp"
            | "image/gif"
    )
}

fn extension_for_mime(value: &str) -> &'static str {
    match value {
        "video/mp4" => "mp4",
        "video/quicktime" => "mov",
        "video/webm" => "webm",
        "audio/mpeg" => "mp3",
        "audio/wav" => "wav",
        "audio/mp4" => "m4a",
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        "image/gif" => "gif",
        _ => "bin",
    }
}

fn ffprobe_path() -> PathBuf {
    // GUI 启动的 App 没有 shell PATH；沿用 executor ToolDiscoveryConfig 的可信目录。
    for directory in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"] {
        let candidate = Path::new(directory).join("ffprobe");
        if candidate.is_file() {
            return candidate;
        }
    }
    PathBuf::from("ffprobe")
}

pub(crate) fn probe_media(path: &Path) -> Result<MediaProbeSummary, String> {
    let output = Command::new(ffprobe_path())
        .args([
            "-v",
            "error",
            "-show_streams",
            "-show_format",
            "-of",
            "json",
        ])
        .arg(path)
        .output()
        .map_err(|error| format!("cannot start ffprobe: {error}"))?;
    if !output.status.success() || output.stdout.len() > 4 * 1024 * 1024 {
        return Err("ffprobe could not inspect selected local media".to_owned());
    }
    let value: Value = serde_json::from_slice(&output.stdout)
        .map_err(|_| "ffprobe returned invalid media metadata".to_owned())?;
    let video = value
        .get("streams")
        .and_then(Value::as_array)
        .and_then(|streams| {
            streams
                .iter()
                .find(|stream| stream.get("codec_type").and_then(Value::as_str) == Some("video"))
        });
    let duration_seconds = value
        .pointer("/format/duration")
        .and_then(Value::as_str)
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value >= 0.0);
    Ok(MediaProbeSummary {
        width: video
            .and_then(|stream| stream.get("width"))
            .and_then(Value::as_u64),
        height: video
            .and_then(|stream| stream.get("height"))
            .and_then(Value::as_u64),
        duration_ms: duration_seconds.map(|value| (value * 1000.0).round() as u64),
    })
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let file = File::open(path).map_err(|error| format!("cannot open local media: {error}"))?;
    let mut reader = BufReader::new(file);
    let mut digest = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| format!("cannot hash local media: {error}"))?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

pub(crate) fn copy_verified_file(
    source: &Path,
    target: &Path,
    expected_sha256: &str,
) -> Result<(), String> {
    if target.exists() {
        if sha256_file(target)? == expected_sha256 {
            return Ok(());
        }
        return Err("managed media output conflicts with another file".to_owned());
    }
    let parent = target
        .parent()
        .ok_or_else(|| "managed media target has no parent".to_owned())?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("cannot create managed media directory: {error}"))?;
    let staging = parent.join(format!(
        ".copy-{}-{}.part",
        std::process::id(),
        random_hex(8)?
    ));
    let result = (|| {
        let mut input = File::open(source)
            .map_err(|error| format!("cannot open source media for copy: {error}"))?;
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staging)
            .map_err(|error| format!("cannot create media copy staging file: {error}"))?;
        std::io::copy(&mut input, &mut output)
            .map_err(|error| format!("cannot copy media into project: {error}"))?;
        output
            .sync_all()
            .map_err(|error| format!("cannot sync project media copy: {error}"))?;
        if sha256_file(&staging)? != expected_sha256 {
            return Err("project media copy digest verification failed".to_owned());
        }
        std::fs::rename(&staging, target)
            .map_err(|error| format!("cannot publish project media copy: {error}"))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(staging);
    }
    result
}

fn content_asset_id(sha256: &str) -> String {
    format!("asset-{}", &sha256[..32])
}

fn reference_asset_id(root_id: &str, relative_path: &str, sha256: &str) -> String {
    let digest = Sha256::digest(format!("{root_id}\0{relative_path}\0{sha256}").as_bytes());
    format!("asset-{}", &format!("{digest:x}")[..32])
}

fn load_root_registry(path: &Path) -> Result<RootRegistry, String> {
    if !path.exists() {
        return Ok(RootRegistry {
            version: ROOT_REGISTRY_VERSION,
            roots: HashMap::new(),
            project_media_dirs: HashMap::new(),
        });
    }
    let file = File::open(path)
        .map_err(|error| format!("cannot open local media root registry: {error}"))?;
    let registry: RootRegistry = serde_json::from_reader(BufReader::new(file))
        .map_err(|error| format!("invalid local media root registry: {error}"))?;
    if registry.version != ROOT_REGISTRY_VERSION {
        return Err("local media root registry version is unsupported".to_owned());
    }
    for root_id in registry.roots.keys() {
        validate_root_id(root_id)?;
    }
    for project_id in registry.project_media_dirs.keys() {
        validate_project_id(project_id)?;
    }
    Ok(registry)
}

fn save_root_registry(path: &Path, registry: &RootRegistry) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "local media root registry has no parent".to_owned())?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("cannot create local media registry directory: {error}"))?;
    let staging = parent.join(format!(
        ".local-media-roots-{}-{}.part",
        std::process::id(),
        random_hex(8)?
    ));
    let result = (|| {
        #[cfg(unix)]
        let file = {
            use std::os::unix::fs::OpenOptionsExt;
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&staging)
        };
        #[cfg(not(unix))]
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staging);
        let mut file = file
            .map_err(|error| format!("cannot create local media registry staging file: {error}"))?;
        serde_json::to_writer_pretty(&mut file, registry)
            .map_err(|error| format!("cannot encode local media root registry: {error}"))?;
        file.write_all(b"\n")
            .map_err(|error| format!("cannot finish local media root registry: {error}"))?;
        file.sync_all()
            .map_err(|error| format!("cannot sync local media root registry: {error}"))?;
        std::fs::rename(&staging, path)
            .map_err(|error| format!("cannot publish local media root registry: {error}"))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(staging);
    }
    result
}

fn random_hex(bytes: usize) -> Result<String, String> {
    let mut value = vec![0u8; bytes];
    getrandom::fill(&mut value).map_err(|_| "secure randomness is unavailable".to_owned())?;
    Ok(value.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn modified_nanos(metadata: &std::fs::Metadata) -> Result<u128, String> {
    metadata
        .modified()
        .map_err(|error| format!("cannot read local media modification time: {error}"))?
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .map_err(|_| "local media modification time is invalid".to_owned())
}

fn path_to_private_string(path: &Path) -> Result<String, String> {
    path.to_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| "local media root is not valid UTF-8".to_owned())
}

fn relative_path_string(path: &Path) -> Result<String, String> {
    validate_relative_path(path)?;
    path.to_str()
        .map(|value| value.replace('\\', "/"))
        .ok_or_else(|| "local media relative path is not valid UTF-8".to_owned())
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0u8, |difference, (left, right)| difference | (left ^ right))
        == 0
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn validate_archive_limits<R: Read + Seek>(archive: &mut ZipArchive<R>) -> Result<(), String> {
    if archive.is_empty() || archive.len() > MAX_ARCHIVE_ENTRIES {
        return Err("canvas ZIP entry count crossed the fixed boundary".to_owned());
    }
    let total = (0..archive.len()).try_fold(0u64, |total, index| {
        let entry = archive
            .by_index_raw(index)
            .map_err(|error| format!("cannot inspect canvas ZIP entry: {error}"))?;
        total
            .checked_add(entry.size())
            .ok_or_else(|| "canvas ZIP uncompressed size overflow".to_owned())
    })?;
    if total > MAX_ARCHIVE_UNCOMPRESSED_BYTES {
        return Err("canvas ZIP uncompressed bytes crossed the fixed boundary".to_owned());
    }
    Ok(())
}

fn validate_archive_relative_path(value: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > 2048 || value.chars().any(char::is_control) {
        return Err("canvas ZIP path is invalid".to_owned());
    }
    validate_relative_path(Path::new(value))
}

fn validate_embedded_media_path(value: &str) -> Result<(), String> {
    validate_archive_relative_path(value)?;
    let parts = value.split('/').collect::<Vec<_>>();
    if parts.len() < 4
        || parts[0] != "projects"
        || parts[1].is_empty()
        || parts[2] != "files"
        || parts[3..].iter().any(|part| part.is_empty())
    {
        return Err("desktop export media path is outside the project files boundary".to_owned());
    }
    Ok(())
}

fn rewrite_storage_reference(
    value: &mut Value,
    old_storage_key: &str,
    reference: &LocalMediaReference,
) {
    match value {
        Value::Array(items) => items
            .iter_mut()
            .for_each(|item| rewrite_storage_reference(item, old_storage_key, reference)),
        Value::Object(object) => {
            let matches = object
                .get("storageKey")
                .and_then(Value::as_str)
                .is_some_and(|value| value == old_storage_key);
            if matches {
                object.insert(
                    "storageKey".to_owned(),
                    Value::String(reference.storage_key.clone()),
                );
                if object.contains_key("content") {
                    object.insert(
                        "content".to_owned(),
                        Value::String(reference.storage_key.clone()),
                    );
                }
                if object.contains_key("url") {
                    object.insert(
                        "url".to_owned(),
                        Value::String(reference.storage_key.clone()),
                    );
                }
                object.insert("localMedia".to_owned(), json!(reference));
                return;
            }
            object
                .values_mut()
                .for_each(|item| rewrite_storage_reference(item, old_storage_key, reference));
        }
        _ => {}
    }
}

fn validate_zip_target(target: &Path) -> Result<(), String> {
    if target
        .extension()
        .and_then(|value| value.to_str())
        .is_none_or(|value| !value.eq_ignore_ascii_case("zip"))
    {
        return Err("canvas exports must use the .zip extension".to_owned());
    }
    if target.exists() {
        return Err("the selected export path already exists; choose a new file name".to_owned());
    }
    Ok(())
}

fn unique_staging_path(target: &Path) -> Result<PathBuf, String> {
    let parent = target
        .parent()
        .ok_or_else(|| "the selected export path has no parent directory".to_owned())?;
    let name = target
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "the selected export file name is invalid".to_owned())?;
    Ok(parent.join(format!(
        ".{name}.part-{}-{}",
        std::process::id(),
        random_hex(8)?
    )))
}

pub(crate) fn parse_export_envelope(
    bytes: &[u8],
) -> Result<Option<(DesktopExportEnvelope, &[u8])>, String> {
    if !bytes.starts_with(EXPORT_ENVELOPE_MAGIC) {
        return Ok(None);
    }
    if bytes.len() < 8 {
        return Err("desktop export envelope is truncated".to_owned());
    }
    let manifest_len = u32::from_be_bytes(bytes[4..8].try_into().unwrap()) as usize;
    let manifest_end = 8usize
        .checked_add(manifest_len)
        .ok_or_else(|| "desktop export envelope size overflow".to_owned())?;
    if manifest_len == 0 || manifest_end >= bytes.len() {
        return Err("desktop export envelope manifest is invalid".to_owned());
    }
    let envelope = serde_json::from_slice::<DesktopExportEnvelope>(&bytes[8..manifest_end])
        .map_err(|error| format!("invalid desktop export envelope: {error}"))?;
    Ok(Some((envelope, &bytes[manifest_end..])))
}

#[tauri::command]
pub(crate) async fn select_local_media(
    app: AppHandle,
    manager: TauriState<'_, Arc<LocalMediaManager>>,
    mode: LocalMediaMode,
    project_id: Option<String>,
) -> Result<Vec<LocalMediaImportOutcome>, String> {
    let selected = app
        .dialog()
        .file()
        .set_title(match mode {
            LocalMediaMode::Reference => "引用本机素材（不复制、不上传）",
            LocalMediaMode::ProjectCopy => "复制素材进项目（不上传云端）",
        })
        .add_filter(
            "媒体文件",
            &[
                "mp4", "m4v", "mov", "webm", "mp3", "wav", "m4a", "png", "jpg", "jpeg", "webp",
                "gif",
            ],
        )
        .blocking_pick_files()
        .unwrap_or_default();
    let paths = selected
        .into_iter()
        .map(|selected| match selected {
            FilePath::Path(path) => Ok(path),
            FilePath::Url(_) => Err("URL media selections are not supported".to_owned()),
        })
        .collect::<Result<Vec<_>, _>>()?;
    import_paths_blocking(manager.inner().clone(), project_id, paths, mode).await
}

/// Imports files the window received through the native drag-and-drop event (real paths, no
/// browser upload); shares the collection policy with the file picker.
#[tauri::command]
pub(crate) async fn import_local_media_paths(
    manager: TauriState<'_, Arc<LocalMediaManager>>,
    project_id: Option<String>,
    paths: Vec<String>,
    mode: LocalMediaMode,
) -> Result<Vec<LocalMediaImportOutcome>, String> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    if paths.len() > MAX_IMPORT_PATHS {
        return Err(format!("at most {MAX_IMPORT_PATHS} files can be imported at once"));
    }
    let paths = paths.into_iter().map(PathBuf::from).collect();
    import_paths_blocking(manager.inner().clone(), project_id, paths, mode).await
}

async fn import_paths_blocking(
    manager: Arc<LocalMediaManager>,
    project_id: Option<String>,
    paths: Vec<PathBuf>,
    mode: LocalMediaMode,
) -> Result<Vec<LocalMediaImportOutcome>, String> {
    if let Some(project_id) = project_id.as_deref() {
        validate_project_id(project_id)?;
    }
    tauri::async_runtime::spawn_blocking(move || {
        paths
            .iter()
            .map(|path| manager.import_path(project_id.as_deref(), path, mode))
            .collect()
    })
    .await
    .map_err(|_| "local media import could not complete".to_owned())?
}

#[tauri::command]
pub(crate) fn project_media_directory(
    manager: TauriState<'_, Arc<LocalMediaManager>>,
    project_id: String,
) -> Result<Option<String>, String> {
    validate_project_id(&project_id)?;
    Ok(manager
        .project_media_directory(&project_id)
        .map(|path| path.to_string_lossy().into_owned()))
}

#[tauri::command]
pub(crate) async fn select_project_media_directory(
    app: AppHandle,
    manager: TauriState<'_, Arc<LocalMediaManager>>,
    project_id: String,
) -> Result<Option<String>, String> {
    validate_project_id(&project_id)?;
    let selected = app
        .dialog()
        .file()
        .set_title("选择这张画布的素材目录（临时文件会被收进这里的「画布素材」）")
        .blocking_pick_folder();
    let Some(selected) = selected else {
        return Ok(None);
    };
    let path = match selected {
        FilePath::Path(path) => path,
        FilePath::Url(_) => return Err("URL directories are not supported".to_owned()),
    };
    let manager = manager.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        manager.set_project_media_directory(&project_id, &path)
    })
    .await
    .map_err(|_| "project media directory selection could not complete".to_owned())?
    .map(Some)
}

#[tauri::command]
pub(crate) async fn resolve_local_media_reference(
    manager: TauriState<'_, Arc<LocalMediaManager>>,
    reference: LocalMediaReference,
    project_id: Option<String>,
) -> Result<LocalMediaResolution, String> {
    if let Some(project_id) = project_id.as_deref() {
        validate_project_id(project_id)?;
    }
    let manager = manager.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        manager.resolve_reference_for_project(project_id.as_deref(), reference)
    })
    .await
    .map_err(|_| "local media resolution could not complete".to_owned())
}

#[tauri::command]
pub(crate) async fn relink_local_media_reference(
    app: AppHandle,
    manager: TauriState<'_, Arc<LocalMediaManager>>,
    reference: LocalMediaReference,
) -> Result<Option<LocalMediaResolution>, String> {
    let selected = app
        .dialog()
        .file()
        .set_title("重新定位同一份本机素材")
        .blocking_pick_file();
    let Some(selected) = selected else {
        return Ok(None);
    };
    let path = match selected {
        FilePath::Path(path) => path,
        FilePath::Url(_) => return Err("URL media selections are not supported".to_owned()),
    };
    let manager = manager.inner().clone();
    tauri::async_runtime::spawn_blocking(move || manager.relink_reference(reference, &path))
        .await
        .map_err(|_| "local media relink could not complete".to_owned())?
        .map(Some)
}

#[tauri::command]
pub(crate) fn local_media_request_evidence(
    manager: TauriState<'_, Arc<LocalMediaManager>>,
) -> Vec<LocalMediaRequestEvidence> {
    manager.request_evidence()
}

#[tauri::command]
pub(crate) async fn import_canvas_archive(
    app: AppHandle,
    manager: TauriState<'_, Arc<LocalMediaManager>>,
) -> Result<Option<DesktopCanvasImportResult>, String> {
    let selected = app
        .dialog()
        .file()
        .set_title("导入无限画布项目")
        .add_filter("ZIP archive", &["zip"])
        .blocking_pick_file();
    let Some(selected) = selected else {
        return Ok(None);
    };
    let path = match selected {
        FilePath::Path(path) => path,
        FilePath::Url(_) => return Err("URL canvas archives are not supported".to_owned()),
    };
    let manager = manager.inner().clone();
    tauri::async_runtime::spawn_blocking(move || manager.import_archive(&path))
        .await
        .map_err(|_| "canvas archive import could not complete".to_owned())?
        .map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tempfile::TempDir;
    use tower::ServiceExt;

    fn test_manager(root: &TempDir) -> Arc<LocalMediaManager> {
        let managed = root.path().join("managed");
        std::fs::create_dir_all(managed.join("owned")).unwrap();
        let roots_path = root.path().join("roots.json");
        let roots = RootRegistry {
            version: ROOT_REGISTRY_VERSION,
            roots: HashMap::from([
                (
                    "fixture-root".to_owned(),
                    root.path().to_str().unwrap().to_owned(),
                ),
                (
                    MANAGED_ROOT_ID.to_owned(),
                    managed.to_str().unwrap().to_owned(),
                ),
            ]),
            project_media_dirs: HashMap::new(),
        };
        save_root_registry(&roots_path, &roots).unwrap();
        Arc::new(LocalMediaManager {
            app_data_directory: root.path().to_path_buf(),
            roots_path,
            managed_root: managed,
            roots: Mutex::new(roots),
            assets: RwLock::new(HashMap::new()),
            credential: "0123456789abcdef0123456789abcdef".to_owned(),
            port: 3999,
            web_port: 3210,
            evidence: Mutex::new(Vec::new()),
            shutdown: Mutex::new(None),
        })
    }

    fn fixture_reference(bytes: &[u8]) -> LocalMediaReference {
        let sha256 = format!("{:x}", Sha256::digest(bytes));
        let asset_id = reference_asset_id("fixture-root", "clip.mp4", &sha256);
        LocalMediaReference {
            storage_key: format!("local-ref:{asset_id}"),
            asset_id,
            root_id: "fixture-root".to_owned(),
            relative_path: "clip.mp4".to_owned(),
            sha256,
            mime_type: "video/mp4".to_owned(),
            bytes: bytes.len() as u64,
            file_name: "clip.mp4".to_owned(),
            width: Some(320),
            height: Some(180),
            duration_ms: Some(1000),
            mode: LocalMediaMode::Reference,
        }
    }

    fn zip_fixture(manifest: &Value, media_path: Option<&str>, media: &[u8]) -> Vec<u8> {
        let cursor = std::io::Cursor::new(Vec::new());
        let mut writer = ZipWriter::new(cursor);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        writer.start_file("projects.json", options).unwrap();
        writer
            .write_all(serde_json::to_string(manifest).unwrap().as_bytes())
            .unwrap();
        if let Some(path) = media_path {
            writer.start_file(path, options).unwrap();
            writer.write_all(media).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn range_parser_accepts_open_closed_and_suffix_ranges() {
        assert_eq!(
            parse_single_range("bytes=0-3", 10),
            Ok(ByteRange { start: 0, end: 3 })
        );
        assert_eq!(
            parse_single_range("bytes=4-", 10),
            Ok(ByteRange { start: 4, end: 9 })
        );
        assert_eq!(
            parse_single_range("bytes=-3", 10),
            Ok(ByteRange { start: 7, end: 9 })
        );
        assert!(parse_single_range("bytes=10-", 10).is_err());
        assert!(parse_single_range("bytes=0-1,4-5", 10).is_err());
    }

    #[tokio::test]
    async fn media_route_requires_authentication_and_returns_real_206_ranges() {
        let root = TempDir::new().unwrap();
        let bytes = b"0123456789";
        let path = root.path().join("clip.mp4");
        std::fs::write(&path, bytes).unwrap();
        let manager = test_manager(&root);
        let reference = fixture_reference(bytes);
        manager
            .register_verified_reference(reference.clone(), path)
            .unwrap();
        let router = media_router(manager.clone());

        let missing_credential = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/v1/media/{}", reference.asset_id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(missing_credential.status(), StatusCode::UNAUTHORIZED);

        let unauthorized = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/v1/media/{}?token=wrong", reference.asset_id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let response = router
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/v1/media/{}?token={}",
                        reference.asset_id, manager.credential
                    ))
                    .header(RANGE, "bytes=2-5")
                    .header(ORIGIN, "http://127.0.0.1:3210")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(response.headers()[CONTENT_RANGE], "bytes 2-5/10");
        assert_eq!(response.headers()[ACCEPT_RANGES], "bytes");
        assert_eq!(
            response.into_body().collect().await.unwrap().to_bytes(),
            "2345"
        );
        assert!(manager
            .request_evidence()
            .iter()
            .any(|record| record.status == 206 && record.response_bytes == 4));
    }

    #[test]
    fn resolving_rejects_traversal_symlinks_and_digest_mismatches() {
        let root = TempDir::new().unwrap();
        let manager = test_manager(&root);
        let bytes = b"fixture";
        std::fs::write(root.path().join("clip.mp4"), bytes).unwrap();
        let valid = fixture_reference(bytes);
        assert_eq!(manager.resolve_reference(valid.clone()).status, "available");

        let mut traversal = valid.clone();
        traversal.relative_path = "../clip.mp4".to_owned();
        assert_eq!(manager.resolve_reference(traversal).reason, Some("denied"));

        let mut mismatch = valid.clone();
        mismatch.sha256 = "a".repeat(64);
        mismatch.asset_id =
            reference_asset_id(&mismatch.root_id, &mismatch.relative_path, &mismatch.sha256);
        mismatch.storage_key = format!("local-ref:{}", mismatch.asset_id);
        assert_eq!(
            manager.resolve_reference(mismatch).reason,
            Some("digest_mismatch")
        );

        let mut forged = valid.clone();
        forged.asset_id = "asset-missing0123456789abcdef012345".to_owned();
        forged.storage_key = format!("local-ref:{}", forged.asset_id);
        assert_eq!(manager.resolve_reference(forged).reason, Some("denied"));

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.path().join("clip.mp4"), root.path().join("link.mp4"))
                .unwrap();
            let mut symlink = valid;
            symlink.relative_path = "link.mp4".to_owned();
            symlink.asset_id =
                reference_asset_id(&symlink.root_id, &symlink.relative_path, &symlink.sha256);
            symlink.storage_key = format!("local-ref:{}", symlink.asset_id);
            assert_eq!(manager.resolve_reference(symlink).reason, Some("denied"));
        }
    }

    #[test]
    fn missing_media_has_structured_relink_state_without_a_path() {
        let root = TempDir::new().unwrap();
        let manager = test_manager(&root);
        let resolution = manager.resolve_reference(fixture_reference(b"missing"));
        assert_eq!(resolution.status, "missing");
        assert_eq!(resolution.reason, Some("missing"));
        assert!(resolution.playback_url.is_none());
        let encoded = serde_json::to_string(&resolution).unwrap();
        assert!(!encoded.contains(root.path().to_str().unwrap()));
    }

    #[test]
    fn reference_and_project_copy_modes_are_explicit_and_content_addressed() {
        let root = TempDir::new().unwrap();
        let source = root.path().join("source.mp4");
        std::fs::write(&source, b"local-media-mode-fixture").unwrap();
        let manager = test_manager(&root);

        let referenced = manager
            .register_selected_path(&source, LocalMediaMode::Reference)
            .unwrap();
        assert_eq!(referenced.reference.mode, LocalMediaMode::Reference);
        assert_ne!(referenced.reference.root_id, MANAGED_ROOT_ID);
        assert_eq!(referenced.reference.relative_path, "source.mp4");
        assert!(referenced.playback_url.is_some());

        let copied = manager
            .register_selected_path(&source, LocalMediaMode::ProjectCopy)
            .unwrap();
        assert_eq!(copied.reference.mode, LocalMediaMode::ProjectCopy);
        assert_eq!(copied.reference.root_id, MANAGED_ROOT_ID);
        assert!(copied.reference.relative_path.starts_with("owned/asset-"));
        let copied_path = manager.managed_root.join(&copied.reference.relative_path);
        assert_eq!(
            std::fs::read(copied_path).unwrap(),
            b"local-media-mode-fixture"
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&manager.roots_path)
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600);
        }
    }

    #[test]
    fn temporary_sources_are_detected_by_location_or_screenshot_name() {
        let home = Path::new("/Users/tester");
        let app_data =
            Path::new("/Users/tester/Library/Application Support/com.chenyuxiaojin.infinitecanvas");
        let temp = |value: &str| is_temporary_source_with_home(Path::new(value), app_data, Some(home));
        assert!(temp("/private/var/folders/ab/T/TemporaryItems/截屏.png"));
        assert!(temp("/Users/tester/Library/Containers/com.tencent.xinWeChat/Data/Documents/xwechat_files/x/temp/RWTemp/2026-09/a.jpg"));
        assert!(temp("/Users/tester/Downloads/photo.jpg"));
        assert!(temp("/Users/tester/.Trash/old.png"));
        assert!(temp("/Users/tester/Desktop/截屏2026-09-02 19.20.11.png"));
        assert!(temp("/Users/tester/Desktop/Screenshot 2026-09-02 at 19.20.11.png"));
        assert!(temp("/Users/tester/Desktop/CleanShot 2026-09-02.png"));
        assert!(!temp("/Users/tester/Library/Application Support/com.chenyuxiaojin.infinitecanvas/agent-media/verified/agent-image-abc.png"));
        assert!(!temp("/Users/tester/Library/Mobile Documents/com~apple~CloudDocs/素材/a.png"));
        assert!(!temp("/Users/tester/Library/CloudStorage/Dropbox/a.png"));
        assert!(!temp("/Users/tester/项目/视频制作台/AI编导/案例2/02-关键帧/定妆/S01.png"));
        assert!(!temp("/Users/tester/Desktop/定妆照.png"));
    }

    #[test]
    fn temporary_media_is_moved_into_the_project_media_folder_and_deduplicated() {
        let root = TempDir::new().unwrap();
        let manager = test_manager(&root);
        let workspace = TempDir::new().unwrap();
        let project_dir = workspace.path().join("案例");
        let inbox = workspace.path().join("inbox");
        std::fs::create_dir_all(&project_dir).unwrap();
        std::fs::create_dir_all(&inbox).unwrap();
        manager
            .set_project_media_directory("case-test", &project_dir)
            .unwrap();
        let folder = project_dir.join(PROJECT_MEDIA_SUBDIRECTORY);

        let shot = inbox.join("截屏2026-09-02 19.20.11.png");
        std::fs::write(&shot, b"shot-a").unwrap();
        let moved = manager
            .import_path_with_policy(Some("case-test"), &shot, LocalMediaMode::Reference, true)
            .unwrap();
        assert_eq!(moved.action, LocalMediaImportAction::Moved);
        assert_eq!(moved.destination, LocalMediaImportDestination::ProjectDirectory);
        assert!(moved.temporary_source);
        assert!(!shot.exists());
        assert!(folder.join("截屏2026-09-02 19.20.11.png").is_file());
        assert_eq!(moved.resolution.status, "available");
        assert_eq!(moved.resolution.reference.mode, LocalMediaMode::Reference);
        assert_eq!(moved.resolution.reference.file_name, "截屏2026-09-02 19.20.11.png");
        assert_eq!(moved.resolution.reference.relative_path, "截屏2026-09-02 19.20.11.png");

        let duplicate = inbox.join("copy.png");
        std::fs::write(&duplicate, b"shot-a").unwrap();
        let reused = manager
            .import_path_with_policy(Some("case-test"), &duplicate, LocalMediaMode::Reference, true)
            .unwrap();
        assert!(!duplicate.exists());
        assert_eq!(reused.resolution.reference.sha256, moved.resolution.reference.sha256);
        assert_eq!(reused.resolution.reference.file_name, "截屏2026-09-02 19.20.11.png");
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 1);

        let clash = inbox.join("截屏2026-09-02 19.20.11.png");
        std::fs::write(&clash, b"shot-b").unwrap();
        let suffixed = manager
            .import_path_with_policy(Some("case-test"), &clash, LocalMediaMode::Reference, true)
            .unwrap();
        assert!(folder.join("截屏2026-09-02 19.20.11-2.png").is_file());
        assert_eq!(suffixed.resolution.reference.file_name, "截屏2026-09-02 19.20.11-2.png");

        let durable = workspace.path().join("素材").join("定妆.png");
        std::fs::create_dir_all(durable.parent().unwrap()).unwrap();
        std::fs::write(&durable, b"durable").unwrap();
        let referenced = manager
            .import_path_with_policy(Some("case-test"), &durable, LocalMediaMode::Reference, false)
            .unwrap();
        assert_eq!(referenced.action, LocalMediaImportAction::Referenced);
        assert_eq!(referenced.destination, LocalMediaImportDestination::InPlace);
        assert!(durable.is_file());

        let stray = inbox.join("stray.png");
        std::fs::write(&stray, b"stray").unwrap();
        let managed = manager
            .import_path_with_policy(None, &stray, LocalMediaMode::Reference, true)
            .unwrap();
        assert_eq!(managed.action, LocalMediaImportAction::Moved);
        assert_eq!(managed.destination, LocalMediaImportDestination::ManagedRoot);
        assert!(!stray.exists());
        assert_eq!(managed.resolution.reference.root_id, MANAGED_ROOT_ID);

        let explicit = workspace.path().join("素材").join("copy-me.png");
        std::fs::write(&explicit, b"explicit").unwrap();
        let copied = manager
            .import_path_with_policy(Some("case-test"), &explicit, LocalMediaMode::ProjectCopy, false)
            .unwrap();
        assert_eq!(copied.action, LocalMediaImportAction::Copied);
        assert!(explicit.is_file());

        let registry = load_root_registry(&manager.roots_path).unwrap();
        assert_eq!(
            registry.project_media_dirs.get("case-test").map(String::as_str),
            project_dir.canonicalize().unwrap().to_str()
        );
    }

    #[test]
    fn missing_references_are_relocated_by_digest_inside_the_project_directory() {
        let root = TempDir::new().unwrap();
        let manager = test_manager(&root);
        let workspace = TempDir::new().unwrap();
        let project_dir = workspace.path().join("案例");
        std::fs::create_dir_all(project_dir.join("02-关键帧")).unwrap();
        manager
            .set_project_media_directory("case-test", &project_dir)
            .unwrap();
        let original = project_dir.join("02-关键帧").join("S01.png");
        std::fs::write(&original, b"frame-one").unwrap();
        let referenced = manager
            .register_selected_path(&original, LocalMediaMode::Reference)
            .unwrap();

        let moved_dir = project_dir.join("03-生成").join("选定");
        std::fs::create_dir_all(&moved_dir).unwrap();
        std::fs::write(moved_dir.join("S01.png"), b"decoy-different-content").unwrap();
        let moved = moved_dir.join("S01-final.png");
        std::fs::rename(&original, &moved).unwrap();

        let relocated =
            manager.resolve_reference_for_project(Some("case-test"), referenced.reference.clone());
        assert_eq!(relocated.status, "available");
        assert!(relocated.playback_url.is_some());
        assert_ne!(relocated.reference.asset_id, referenced.reference.asset_id);
        assert_eq!(relocated.reference.file_name, "S01-final.png");
        assert_eq!(relocated.reference.sha256, referenced.reference.sha256);
        assert_eq!(relocated.reference.mode, LocalMediaMode::Reference);

        let direct =
            manager.resolve_reference_for_project(Some("case-test"), relocated.reference.clone());
        assert_eq!(direct.status, "available");
        assert_eq!(direct.reference, relocated.reference);

        let without_project = manager.resolve_reference(referenced.reference.clone());
        assert_eq!(without_project.status, "missing");

        std::fs::remove_file(&moved).unwrap();
        let gone =
            manager.resolve_reference_for_project(Some("case-test"), relocated.reference.clone());
        assert_eq!(gone.status, "missing");
        assert_eq!(gone.reason, Some("missing"));
    }

    #[test]
    fn v3_v4_embedded_import_and_v5_streamed_export_round_trip_without_webview_blobs() {
        let root = TempDir::new().unwrap();
        let manager = test_manager(&root);
        let media = b"zip-round-trip-media";
        let media_path = "projects/project-1/files/legacy.mp4";
        let mut last_project = Value::Null;

        for version in [3, 4] {
            let manifest = json!({
                "app": "infinite-canvas",
                "version": version,
                "exportedAt": "2026-08-31T00:00:00.000Z",
                "projects": [{
                    "project": {
                        "id": "project-1",
                        "nodes": [{
                            "id": "video-1",
                            "metadata": {
                                "content": "media:legacy",
                                "storageKey": "media:legacy"
                            }
                        }],
                        "operationState": {
                            "revision": 7,
                            "audit": [{
                                "snapshot": {
                                    "storageKey": "media:legacy"
                                }
                            }]
                        }
                    },
                    "files": [{
                        "storageKey": "media:legacy",
                        "path": media_path,
                        "mimeType": "video/mp4",
                        "bytes": media.len()
                    }]
                }]
            });
            let archive_path = root.path().join(format!("legacy-v{version}.zip"));
            std::fs::write(
                &archive_path,
                zip_fixture(&manifest, Some(media_path), media),
            )
            .unwrap();
            let imported = manager.import_archive(&archive_path).unwrap();
            assert_eq!(imported.source_version, version);
            assert_eq!(imported.imported_media, 1);
            assert_eq!(
                imported.projects[0].pointer("/operationState/revision"),
                Some(&json!(7))
            );
            let encoded = serde_json::to_string(&imported.projects[0]).unwrap();
            assert!(!encoded.contains("media:legacy"));
            assert!(encoded.contains("local-ref:asset-"));
            last_project = imported.projects[0].clone();
        }

        let reference: LocalMediaReference = serde_json::from_value(
            last_project
                .pointer("/nodes/0/metadata/localMedia")
                .unwrap()
                .clone(),
        )
        .unwrap();
        let v5_manifest = json!({
            "app": "infinite-canvas",
            "version": 5,
            "exportedAt": "2026-08-31T00:00:00.000Z",
            "mediaMode": "embedded",
            "projects": [{
                "project": last_project,
                "files": [{
                    "storageKey": reference.storage_key,
                    "path": media_path,
                    "mimeType": reference.mime_type,
                    "bytes": reference.bytes,
                    "embedded": true,
                    "reference": reference
                }]
            }]
        });
        let base_zip = zip_fixture(&v5_manifest, None, &[]);
        let target = root.path().join("portable-v5.zip");
        manager
            .export_archive(
                &target,
                &base_zip,
                DesktopExportEnvelope {
                    version: EXPORT_ENVELOPE_VERSION,
                    local_files: vec![DesktopExportLocalFile {
                        path: media_path.to_owned(),
                        reference: reference.clone(),
                    }],
                },
            )
            .unwrap();
        let imported_v5 = manager.import_archive(&target).unwrap();
        assert_eq!(imported_v5.source_version, 5);
        assert_eq!(imported_v5.imported_media, 1);
        let round_trip_reference: LocalMediaReference = serde_json::from_value(
            imported_v5.projects[0]
                .pointer("/nodes/0/metadata/localMedia")
                .unwrap()
                .clone(),
        )
        .unwrap();
        assert_eq!(round_trip_reference.sha256, reference.sha256);
        assert_eq!(
            manager.resolve_reference(round_trip_reference).status,
            "available"
        );

        let owned_entries = std::fs::read_dir(manager.managed_root.join("owned"))
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
            .count();
        assert_eq!(owned_entries, 1, "same digest must remain idempotent");
    }

    #[test]
    fn v5_reference_only_import_preserves_manifest_without_claiming_embedded_media() {
        let root = TempDir::new().unwrap();
        let manager = test_manager(&root);
        let reference = fixture_reference(b"reference-only");
        let manifest = json!({
            "app": "infinite-canvas",
            "version": 5,
            "mediaMode": "references",
            "projects": [{
                "project": {
                    "id": "project-1",
                    "nodes": [{ "id": "video-1", "metadata": {
                        "storageKey": reference.storage_key,
                        "localMedia": reference
                    }}]
                },
                "files": [{
                    "storageKey": reference.storage_key,
                    "path": "projects/project-1/files/reference.mp4",
                    "mimeType": "video/mp4",
                    "bytes": reference.bytes,
                    "embedded": false,
                    "reference": reference
                }]
            }]
        });
        let archive_path = root.path().join("references-v5.zip");
        std::fs::write(&archive_path, zip_fixture(&manifest, None, &[])).unwrap();
        let imported = manager.import_archive(&archive_path).unwrap();
        assert_eq!(imported.imported_media, 0);
        assert_eq!(
            imported.projects[0].pointer("/nodes/0/metadata/localMedia/relativePath"),
            Some(&json!("clip.mp4"))
        );
    }
}
