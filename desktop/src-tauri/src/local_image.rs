use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

use local_agent_adapter::{CanvasOperationAdapter, ProjectBinding, SqliteCanvasAdapter};
use local_executor::{AllowedRoot, PathPolicy, RootId, ScopedPath};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tauri::{ipc::Response, AppHandle, Manager, State};

use crate::{agent_bridge::DesktopAgentBridge, project_binding::default_workflow_root};

const MAX_IMAGE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_BINDING_BYTES: u64 = 64 * 1024;
static ACTIVE_READS: AtomicUsize = AtomicUsize::new(0);

struct ReadPermit;

impl ReadPermit {
    fn acquire() -> Result<Self, String> {
        ACTIVE_READS
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < 2).then_some(count + 1)
            })
            .map(|_| Self)
            .map_err(|_| "图片读取繁忙，请稍后重试".to_owned())
    }
}

impl Drop for ReadPermit {
    fn drop(&mut self) {
        ACTIVE_READS.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Only the saved project and asset key cross IPC. Paths and file type come from
/// the authoritative document; transient display bytes never enter that document.
#[tauri::command]
pub(crate) async fn read_canvas_local_image(
    app: AppHandle,
    bridge: State<'_, DesktopAgentBridge>,
    project_id: String,
    storage_key: String,
) -> Result<Response, String> {
    let permit = ReadPermit::acquire()?;
    let canvas = bridge.canvas();
    let workflow_root = default_workflow_root()?;
    let media_root = app
        .path()
        .app_data_dir()
        .map_err(|_| "无法定位画布图片目录".to_owned())?
        .join("agent-media");
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        read_registered_image(
            &canvas,
            &workflow_root,
            &media_root,
            &project_id,
            &storage_key,
        )
        .map(Response::new)
    })
    .await
    .map_err(|_| "图片读取任务失败".to_owned())?
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct RegisteredImage {
    asset_id: String,
    storage_key: String,
    root_id: String,
    relative_path: String,
    mime_type: String,
    bytes: u64,
    sha256: String,
}

fn registered_image(
    project: &Value,
    project_id: &str,
    key: &str,
) -> Result<RegisteredImage, String> {
    if project.get("id").and_then(Value::as_str) != Some(project_id)
        || key.len() > 256
        || !key.starts_with("local-ref:")
    {
        return Err("图片引用与当前画布不匹配".to_owned());
    }
    let nodes = project["nodes"]
        .as_array()
        .ok_or_else(|| "画布节点记录无效".to_owned())?;
    let mut registered = None;
    for node in nodes
        .iter()
        .filter(|node| node["metadata"]["storageKey"].as_str() == Some(key))
    {
        if !matches!(node["type"].as_str(), Some("image" | "panorama")) {
            return Err("此素材不是图片节点".to_owned());
        }
        let image: RegisteredImage = serde_json::from_value(node["metadata"]["localMedia"].clone())
            .map_err(|_| "图片素材登记不完整".to_owned())?;
        if image.storage_key != key
            || image.asset_id.is_empty()
            || key != format!("local-ref:{}", image.asset_id)
            || image.root_id != "agent-media"
            || image.bytes == 0
            || image.bytes > MAX_IMAGE_BYTES
            || image.sha256.len() != 64
            || !image.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
            || !matches!(
                image.mime_type.as_str(),
                "image/png" | "image/jpeg" | "image/gif" | "image/webp"
            )
        {
            return Err("图片素材登记越界或格式不支持（单张上限 64 MiB）".to_owned());
        }
        if registered
            .as_ref()
            .is_some_and(|previous| previous != &image)
        {
            return Err("同一图片引用存在冲突登记".to_owned());
        }
        registered = Some(image);
    }
    registered.ok_or_else(|| "当前画布未登记这张图片".to_owned())
}

/// No title matching, setup call, fallback root, or configuration write here.
fn exact_bound_directory(workflow_root: &Path, project_id: &str) -> Result<PathBuf, String> {
    let workflow_root = workflow_root
        .canonicalize()
        .map_err(|_| "片子目录不可用".to_owned())?;
    let mut matched = None;
    for entry in fs::read_dir(&workflow_root).map_err(|_| "片子目录不可用".to_owned())? {
        let entry = entry.map_err(|_| "无法读取片子目录".to_owned())?;
        if !entry.file_type().is_ok_and(|kind| kind.is_dir())
            || entry.file_name().to_string_lossy().starts_with('.')
        {
            continue;
        }
        let directory = entry.path();
        let binding_path = directory.join(".infinite-canvas/project.json");
        let Ok(file) = open_without_symlinks(&binding_path) else {
            continue;
        };
        let Ok(bytes) = read_bounded(file, MAX_BINDING_BYTES) else {
            continue;
        };
        let Ok(binding) = serde_json::from_slice::<ProjectBinding>(&bytes) else {
            continue;
        };
        if binding.project_id != project_id {
            continue;
        }
        if binding.version != 1
            || !Path::new(&binding.project_directory).is_absolute()
            || Path::new(&binding.project_directory)
                .canonicalize()
                .ok()
                .as_ref()
                != Some(&directory)
        {
            return Err("片子绑定目录与登记不一致".to_owned());
        }
        if matched.replace(directory).is_some() {
            return Err("多个片子目录绑定了同一个画布，请先确认绑定".to_owned());
        }
    }
    matched.ok_or_else(|| "当前画布没有准确保存的片子绑定".to_owned())
}

fn read_registered_image(
    canvas: &SqliteCanvasAdapter,
    workflow_root: &Path,
    media_root: &Path,
    project_id: &str,
    key: &str,
) -> Result<Vec<u8>, String> {
    let project = canvas
        .get_project(project_id)
        .map_err(|error| error.to_string())?;
    let image = registered_image(&project.project, project_id, key)?;
    exact_bound_directory(workflow_root, project_id)?;
    let root_id = RootId::new("agent-media").map_err(|error| error.to_string())?;
    let root =
        AllowedRoot::new(root_id.clone(), media_root).map_err(|_| "图片目录不可用".to_owned())?;
    // agent-media itself must not redirect to an unrelated root.
    let expected_root = media_root
        .parent()
        .and_then(|path| path.canonicalize().ok())
        .map(|path| path.join("agent-media"));
    if expected_root.as_deref() != Some(root.canonical_path()) {
        return Err("图片目录不能是外部软链接".to_owned());
    }
    let scoped =
        ScopedPath::new(root_id, &image.relative_path).map_err(|_| "图片路径越界".to_owned())?;
    let candidate = root.canonical_path().join(&scoped.relative);
    PathPolicy::new(vec![root])
        .and_then(|policy| policy.resolve_existing_file(&scoped))
        .map_err(|_| "图片文件不存在或路径越界".to_owned())?;
    // Resolve policy first, then open every original component with O_NOFOLLOW.
    // An attacker swapping a directory/symlink after canonicalization cannot
    // redirect the opened descriptor outside the approved root.
    let file = open_without_symlinks(&candidate)?;
    let bytes = read_bounded(file, MAX_IMAGE_BYTES)?;
    if bytes.len() as u64 != image.bytes
        || format!("{:x}", Sha256::digest(&bytes)) != image.sha256.to_ascii_lowercase()
    {
        return Err("图片原文件与登记的大小或校验值不一致".to_owned());
    }
    if image_mime(&bytes) != Some(image.mime_type.as_str()) {
        return Err("图片真实格式与登记不符".to_owned());
    }
    Ok(bytes)
}

pub(crate) fn read_bounded(file: File, limit: u64) -> Result<Vec<u8>, String> {
    let metadata = file.metadata().map_err(|_| "无法读取图片信息".to_owned())?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err("文件类型无效或超出读取上限".to_owned());
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "图片读取失败".to_owned())?;
    if bytes.len() as u64 > limit || bytes.len() as u64 != metadata.len() {
        return Err("文件在读取时变化或超出读取上限".to_owned());
    }
    Ok(bytes)
}

fn image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Some("image/webp")
    } else {
        None
    }
}

#[cfg(unix)]
pub(crate) fn open_without_symlinks(path: &Path) -> Result<File, String> {
    use std::{
        ffi::CString,
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::ffi::OsStrExt,
        },
        path::Component,
    };
    if !path.is_absolute() {
        return Err("文件路径必须是内部绝对路径".to_owned());
    }
    let mut file = File::open("/").map_err(|_| "无法读取文件根目录".to_owned())?;
    let mut components = path.components().peekable();
    while let Some(component) = components.next() {
        if component == Component::RootDir {
            continue;
        }
        let Component::Normal(name) = component else {
            return Err("文件路径越界".to_owned());
        };
        let name = CString::new(name.as_bytes()).map_err(|_| "文件路径无效".to_owned())?;
        let flags = libc::O_RDONLY
            | libc::O_CLOEXEC
            | libc::O_NOFOLLOW
            | libc::O_NONBLOCK
            | if components.peek().is_some() {
                libc::O_DIRECTORY
            } else {
                0
            };
        // SAFETY: parent fd is alive, name is NUL-terminated, flags do not create
        // a file. Each successful fd is immediately owned and closed by File.
        let fd = unsafe { libc::openat(file.as_raw_fd(), name.as_ptr(), flags) };
        if fd < 0 {
            return Err("文件不存在、无权限或包含软链接".to_owned());
        }
        file = unsafe { File::from_raw_fd(fd) };
    }
    Ok(file)
}

#[cfg(not(unix))]
fn open_without_symlinks(_path: &Path) -> Result<File, String> {
    Err("当前平台尚未支持受限本地图片读取".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    const PROJECT_ID: &str = "film-a";
    const KEY: &str = "local-ref:asset-a";
    const PNG: &[u8] = include_bytes!("../icons/icon.png");

    struct Fixture {
        temp: TempDir,
        canvas: SqliteCanvasAdapter,
        workflow: PathBuf,
        media: PathBuf,
        project: Value,
    }

    impl Fixture {
        fn new() -> Self {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().canonicalize().unwrap();
            let workflow = root.join("films");
            let film = workflow.join("film-a");
            let media = root.join("agent-media");
            fs::create_dir_all(media.join("verified")).unwrap();
            fs::create_dir_all(&film).unwrap();
            Self::bind(&film, PROJECT_ID);
            fs::write(media.join("verified/image.png"), PNG).unwrap();
            let database = root.join("test.db");
            rusqlite::Connection::open(&database)
                .unwrap()
                .execute_batch(
                    "CREATE TABLE canvas_projects (user_id INTEGER NOT NULL, id TEXT NOT NULL,
                 project_data TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
                 deleted_at TEXT NOT NULL, PRIMARY KEY (user_id, id));",
                )
                .unwrap();
            let canvas = SqliteCanvasAdapter::open(database).unwrap();
            let project = json!({
                "id": PROJECT_ID, "title": "Film A", "createdAt": "2026-09-04T00:00:00Z",
                "updatedAt": "2026-09-04T00:00:00Z", "connections": [],
                "nodes": [{ "id": "image-a", "type": "image", "metadata": {
                    "content": KEY, "storageKey": KEY, "localMedia": {
                        "assetId": "asset-a", "storageKey": KEY, "rootId": "agent-media",
                        "relativePath": "verified/image.png", "mimeType": "image/png",
                        "bytes": PNG.len(), "sha256": format!("{:x}", Sha256::digest(PNG))
                    }
                }}]
            });
            canvas.save_human_project(project.clone()).unwrap();
            Self {
                temp,
                canvas,
                workflow,
                media,
                project,
            }
        }

        fn bind(film: &Path, id: &str) {
            fs::create_dir_all(film.join(".infinite-canvas")).unwrap();
            let binding = ProjectBinding::new(id, "same title", film).unwrap();
            fs::write(
                film.join(".infinite-canvas/project.json"),
                serde_json::to_vec(&binding).unwrap(),
            )
            .unwrap();
        }

        fn read(&self) -> Result<Vec<u8>, String> {
            read_registered_image(&self.canvas, &self.workflow, &self.media, PROJECT_ID, KEY)
        }

        fn save(&self, project: Value) {
            self.canvas.save_human_project(project).unwrap();
        }
    }

    #[test]
    fn exact_original_bytes_and_no_database_or_binding_write() {
        let fixture = Fixture::new();
        let db_before = fs::read(fixture.canvas.database_path()).unwrap();
        let binding = fixture
            .workflow
            .join("film-a/.infinite-canvas/project.json");
        let binding_before = fs::read(&binding).unwrap();
        assert_eq!(fixture.read().unwrap(), PNG);
        assert_eq!(fs::read(fixture.canvas.database_path()).unwrap(), db_before);
        assert_eq!(fs::read(binding).unwrap(), binding_before);
        assert!(!fixture.workflow.join("film-a/.mcp.json").exists());
        assert!(!fixture.workflow.join("film-a/.codex").exists());
    }

    #[test]
    fn missing_and_duplicate_bindings_fail_without_title_fallback() {
        let fixture = Fixture::new();
        let film = fixture.workflow.join("film-a");
        Fixture::bind(&film, "different-id");
        assert!(fixture.read().unwrap_err().contains("准确保存"));
        Fixture::bind(&film, PROJECT_ID);
        let second = fixture.workflow.join("film-b");
        fs::create_dir(&second).unwrap();
        Fixture::bind(&second, PROJECT_ID);
        assert!(fixture.read().unwrap_err().contains("多个片子"));
    }

    #[test]
    fn rejects_wrong_binding_directory() {
        let fixture = Fixture::new();
        let path = fixture
            .workflow
            .join("film-a/.infinite-canvas/project.json");
        let mut binding: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        binding["project_directory"] = json!(fixture.media);
        fs::write(path, serde_json::to_vec(&binding).unwrap()).unwrap();
        assert!(fixture.read().is_err());
    }

    #[test]
    fn rejects_unknown_project_key_and_non_image_nodes() {
        let fixture = Fixture::new();
        assert!(read_registered_image(
            &fixture.canvas,
            &fixture.workflow,
            &fixture.media,
            "missing",
            KEY
        )
        .is_err());
        assert!(read_registered_image(
            &fixture.canvas,
            &fixture.workflow,
            &fixture.media,
            PROJECT_ID,
            "local-ref:missing"
        )
        .is_err());
        let mut other = fixture.project.clone();
        other["id"] = json!("film-b");
        other["nodes"] = json!([]);
        fixture.save(other);
        assert!(read_registered_image(
            &fixture.canvas,
            &fixture.workflow,
            &fixture.media,
            "film-b",
            KEY
        )
        .is_err());
        for kind in ["video", "text", "audio"] {
            let mut project = fixture.project.clone();
            project["nodes"][0]["type"] = json!(kind);
            fixture.save(project);
            assert!(fixture.read().is_err());
        }
    }

    #[test]
    fn rejects_invalid_registration_fields_and_paths() {
        let fixture = Fixture::new();
        for (field, value) in [
            ("rootId", json!("project-media")),
            ("storageKey", json!("local-ref:other")),
            ("assetId", json!("other")),
            ("bytes", json!(MAX_IMAGE_BYTES + 1)),
            ("sha256", json!("invalid")),
            ("mimeType", json!("image/svg+xml")),
            ("relativePath", json!("../image.png")),
            (
                "relativePath",
                json!(fixture.media.join("verified/image.png")),
            ),
            ("relativePath", json!("verified/missing.png")),
        ] {
            let mut project = fixture.project.clone();
            project["nodes"][0]["metadata"]["localMedia"][field] = value;
            fixture.save(project);
            assert!(fixture.read().is_err(), "{field}");
        }
    }

    #[test]
    fn allows_identical_copies_and_panorama_but_rejects_conflicting_key() {
        let fixture = Fixture::new();
        let mut project = fixture.project.clone();
        project["nodes"][0]["type"] = json!("panorama");
        let mut copy = project["nodes"][0].clone();
        copy["id"] = json!("image-copy");
        project["nodes"].as_array_mut().unwrap().push(copy);
        fixture.save(project.clone());
        assert_eq!(fixture.read().unwrap(), PNG);
        project["nodes"][1]["metadata"]["localMedia"]["relativePath"] = json!("verified/other.png");
        fixture.save(project);
        assert!(fixture.read().unwrap_err().contains("冲突"));
    }

    #[test]
    fn rejects_changed_file_and_non_raster_content() {
        let fixture = Fixture::new();
        fs::write(fixture.media.join("verified/image.png"), b"different bytes").unwrap();
        assert!(fixture.read().unwrap_err().contains("校验值"));
        for bad in [
            b"<svg>not an image</svg>".as_slice(),
            b"<html>not an image</html>",
            b"garbage",
        ] {
            fs::write(fixture.media.join("verified/image.png"), bad).unwrap();
            let mut project = fixture.project.clone();
            project["nodes"][0]["metadata"]["localMedia"]["bytes"] = json!(bad.len());
            project["nodes"][0]["metadata"]["localMedia"]["sha256"] =
                json!(format!("{:x}", Sha256::digest(bad)));
            fixture.save(project);
            assert!(fixture.read().unwrap_err().contains("真实格式"));
        }
    }

    #[test]
    fn bounds_actual_file_read_even_when_registration_lies() {
        let fixture = Fixture::new();
        File::create(fixture.media.join("verified/image.png"))
            .unwrap()
            .set_len(MAX_IMAGE_BYTES + 1)
            .unwrap();
        assert!(fixture.read().unwrap_err().contains("上限"));
        let file = File::open(fixture.media.join("verified/image.png")).unwrap();
        assert!(read_bounded(file, 8).is_err());
        assert!(read_bounded(File::open(&fixture.media).unwrap(), 8).is_err());
    }

    #[test]
    fn accepts_only_raster_signatures_not_extensions_or_claimed_mime() {
        for (bytes, mime) in [
            (b"\x89PNG\r\n\x1a\n".as_slice(), "image/png"),
            (b"\xff\xd8\xff".as_slice(), "image/jpeg"),
            (b"GIF87a".as_slice(), "image/gif"),
            (b"GIF89a".as_slice(), "image/gif"),
            (b"RIFF\x04\x00\x00\x00WEBP".as_slice(), "image/webp"),
        ] {
            assert_eq!(image_mime(bytes), Some(mime));
            assert_eq!(image_mime(&bytes[..bytes.len() - 1]), None);
        }
        for bytes in [
            b"<svg/>".as_slice(),
            b"<html/>",
            b"RIFFxxxxWAVE",
            b"fake.png",
            b"",
        ] {
            assert_eq!(image_mime(bytes), None);
        }
    }

    #[test]
    fn read_permits_are_bounded_and_released() {
        let first = ReadPermit::acquire().unwrap();
        let second = ReadPermit::acquire().unwrap();
        assert!(ReadPermit::acquire().is_err());
        drop(first);
        let replacement = ReadPermit::acquire().unwrap();
        drop(second);
        drop(replacement);
        assert_eq!(ACTIVE_READS.load(Ordering::Acquire), 0);
    }

    /// Explicit opt-in only. The supplied SQLite snapshot is copied before the
    /// adapter opens it. Real bindings/media are only read, never copied into or
    /// changed inside a film directory. This verifies bytes, not WebView decode.
    #[test]
    #[ignore = "requires explicit snapshot, workflow and media root environment variables"]
    fn real_case4_registered_images_read_only() {
        let source = PathBuf::from(
            std::env::var_os("IC_IMAGE_AUDIT_DB_SNAPSHOT").expect("snapshot required"),
        );
        let workflow = PathBuf::from(
            std::env::var_os("IC_IMAGE_AUDIT_WORKFLOW_ROOT").expect("workflow root required"),
        );
        let media = PathBuf::from(
            std::env::var_os("IC_IMAGE_AUDIT_MEDIA_ROOT").expect("media root required"),
        );
        let project_id = "DUkqxVcwRh30uwMAskyxt";
        let temp = tempfile::tempdir().unwrap();
        let database = temp.path().join("case4-read-only.db");
        let source_before = Sha256::digest(fs::read(&source).unwrap());
        fs::copy(&source, &database).unwrap();
        let canvas = SqliteCanvasAdapter::open(&database).unwrap();
        let database_before = Sha256::digest(fs::read(&database).unwrap());
        let directory = exact_bound_directory(&workflow, project_id).unwrap();
        let binding_path = directory.join(".infinite-canvas/project.json");
        let binding_before = fs::read(&binding_path).unwrap();
        let project_before = canvas.get_project(project_id).unwrap();
        let images: Vec<_> = project_before.project["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|node| node["type"] == "image")
            .collect();
        assert_eq!(images.len(), 32);
        let started = std::time::Instant::now();
        let mut total = 0usize;
        for node in &images {
            let key = node["metadata"]["storageKey"].as_str().unwrap();
            let bytes = read_registered_image(&canvas, &workflow, &media, project_id, key).unwrap();
            total += bytes.len();
            println!(
                "verified node={} bytes={} sha256={:x}",
                node["id"].as_str().unwrap(),
                bytes.len(),
                Sha256::digest(&bytes)
            );
        }
        assert_eq!(total, 427_634_172);
        assert_eq!(
            Sha256::digest(fs::read(&database).unwrap()),
            database_before
        );
        assert_eq!(Sha256::digest(fs::read(&source).unwrap()), source_before);
        assert_eq!(fs::read(binding_path).unwrap(), binding_before);
        assert_eq!(
            canvas.get_project(project_id).unwrap().project,
            project_before.project
        );
        println!("verified {} original images / {} bytes in {:?}; source snapshot, database copy and exact binding unchanged; no GUI/decode claim", images.len(), total, started.elapsed());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_files_parents_roots_and_binding() {
        use std::os::unix::fs::symlink;
        let fixture = Fixture::new();
        let outside = fixture.temp.path().canonicalize().unwrap().join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("image.png"), PNG).unwrap();
        fs::remove_file(fixture.media.join("verified/image.png")).unwrap();
        symlink(
            outside.join("image.png"),
            fixture.media.join("verified/image.png"),
        )
        .unwrap();
        assert!(fixture.read().is_err());
        fs::remove_file(fixture.media.join("verified/image.png")).unwrap();
        fs::remove_dir(fixture.media.join("verified")).unwrap();
        symlink(&outside, fixture.media.join("verified")).unwrap();
        assert!(fixture.read().is_err());
        // Even an in-root symlink is rejected by descriptor traversal, not just
        // an escaping one rejected by PathPolicy.
        fs::remove_file(fixture.media.join("verified")).unwrap();
        fs::create_dir(fixture.media.join("real")).unwrap();
        fs::write(fixture.media.join("real/image.png"), PNG).unwrap();
        symlink("real", fixture.media.join("verified")).unwrap();
        assert!(fixture.read().is_err());
        fs::rename(&fixture.media, &outside.join("moved-media")).unwrap();
        symlink(outside.join("moved-media"), &fixture.media).unwrap();
        assert!(fixture.read().unwrap_err().contains("外部软链接"));
        fs::remove_file(&fixture.media).unwrap();
        fs::rename(outside.join("moved-media"), &fixture.media).unwrap();
        let binding = fixture
            .workflow
            .join("film-a/.infinite-canvas/project.json");
        let copy = outside.join("binding.json");
        fs::rename(&binding, &copy).unwrap();
        symlink(copy, binding).unwrap();
        assert!(fixture.read().unwrap_err().contains("准确保存"));
    }

    #[cfg(unix)]
    #[test]
    fn descriptor_remains_original_when_path_is_replaced() {
        use std::os::unix::fs::symlink;
        let fixture = Fixture::new();
        let path = fixture.media.join("verified/image.png");
        let file = open_without_symlinks(&path).unwrap();
        fs::rename(&path, fixture.media.join("verified/original.png")).unwrap();
        let other = fixture
            .temp
            .path()
            .canonicalize()
            .unwrap()
            .join("outside.png");
        fs::write(&other, b"other image").unwrap();
        symlink(other, &path).unwrap();
        assert_eq!(read_bounded(file, MAX_IMAGE_BYTES).unwrap(), PNG);
        assert!(open_without_symlinks(&path).is_err());
    }
}
