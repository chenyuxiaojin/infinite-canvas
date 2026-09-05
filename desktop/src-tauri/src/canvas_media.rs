use std::{fs, path::Path};

use local_agent_adapter::{CanvasOperationAdapter, SqliteCanvasAdapter};
use local_executor::{AllowedRoot, PathPolicy, RootId, ScopedPath};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tauri::{ipc::Response, AppHandle, Manager, State};

use crate::{agent_bridge::DesktopAgentBridge, local_image::{open_without_symlinks, read_bounded}};

const MAX_MEDIA_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct MediaReference {
    asset_id: String,
    storage_key: String,
    root_id: String,
    relative_path: String,
    mime_type: String,
    bytes: u64,
    sha256: String,
}

fn find_reference(nodes: &Value, key: &str, found: &mut Option<MediaReference>) -> Result<(), String> {
    match nodes {
        Value::Object(object) => {
            if object.get("storageKey").and_then(Value::as_str) == Some(key) && object.contains_key("rootId") {
                let reference: MediaReference = serde_json::from_value(nodes.clone()).map_err(|_| "素材登记不完整")?;
                if found.as_ref().is_some_and(|old| old != &reference) { return Err("同一素材存在冲突登记".into()); }
                *found = Some(reference);
            }
            for value in object.values() { find_reference(value, key, found)?; }
        }
        Value::Array(values) => for value in values { find_reference(value, key, found)?; },
        _ => {}
    }
    Ok(())
}

pub(crate) fn read_registered_media(canvas: &SqliteCanvasAdapter, app_data: &Path, project_id: &str, key: &str) -> Result<Vec<u8>, String> {
    if !key.starts_with("local-ref:") || key.len() > 256 { return Err("素材引用无效".into()); }
    let project = canvas.get_project(project_id).map_err(|error| error.to_string())?;
    let mut found = None;
    find_reference(&project.project["nodes"], key, &mut found)?;
    let reference = found.ok_or("当前画布及其历史版本未登记此素材")?;
    if reference.storage_key != key || key != format!("local-ref:{}", reference.asset_id) || reference.bytes == 0 || reference.bytes > MAX_MEDIA_BYTES
        || reference.sha256.len() != 64 || !reference.sha256.bytes().all(|b| b.is_ascii_hexdigit())
        || !(reference.mime_type.starts_with("image/") || reference.mime_type.starts_with("video/") || reference.mime_type.starts_with("audio/")) {
        return Err("素材登记格式无效或超过 512 MiB".into());
    }
    let registry: Value = serde_json::from_slice(&fs::read(app_data.join("local-media-roots.json")).map_err(|_| "素材目录登记不可用")?).map_err(|_| "素材目录登记无效")?;
    let root_path = registry["roots"][&reference.root_id].as_str().ok_or("素材目录尚未登记")?;
    let root_id = RootId::new(&reference.root_id).map_err(|e| e.to_string())?;
    let root = AllowedRoot::new(root_id.clone(), root_path).map_err(|e| e.to_string())?;
    let scoped = ScopedPath::new(root_id, &reference.relative_path).map_err(|e| e.to_string())?;
    let candidate = root.canonical_path().join(&scoped.relative);
    PathPolicy::new(vec![root]).and_then(|policy| policy.resolve_existing_file(&scoped)).map_err(|_| "素材不存在或路径越界")?;
    let bytes = read_bounded(open_without_symlinks(&candidate)?, MAX_MEDIA_BYTES)?;
    if bytes.len() as u64 != reference.bytes || format!("{:x}", Sha256::digest(&bytes)) != reference.sha256.to_ascii_lowercase() {
        return Err("素材原文件与登记的大小或校验值不一致".into());
    }
    Ok(bytes)
}

#[tauri::command]
pub(crate) async fn read_canvas_local_media(app: AppHandle, bridge: State<'_, DesktopAgentBridge>, project_id: String, storage_key: String) -> Result<Response, String> {
    let canvas = bridge.canvas();
    let directory = app.path().app_data_dir().map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || read_registered_media(&canvas, &directory, &project_id, &storage_key).map(Response::new))
        .await.map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn history_media_is_scoped_and_verified_without_database_writes() {
        let directory = tempfile::tempdir().unwrap();
        let db = directory.path().join("canvas.db");
        let sql = rusqlite::Connection::open(&db).unwrap();
        sql.execute_batch("CREATE TABLE canvas_projects (user_id TEXT NOT NULL, id TEXT NOT NULL, project_data TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL, deleted_at TEXT NOT NULL DEFAULT '', PRIMARY KEY(user_id,id));").unwrap();
        let media = directory.path().join("media");
        fs::create_dir(&media).unwrap();
        fs::write(media.join("original.png"), b"original bytes").unwrap();
        fs::write(directory.path().join("local-media-roots.json"), serde_json::to_vec(&json!({"roots":{"test-media":media}})).unwrap()).unwrap();
        let canvas = SqliteCanvasAdapter::open(&db).unwrap();
        let reference = json!({"assetId":"a","storageKey":"local-ref:a","rootId":"test-media","relativePath":"original.png","mimeType":"image/png","bytes":14,"sha256":format!("{:x}",Sha256::digest(b"original bytes"))});
        let project = json!({"id":"film","title":"film","createdAt":"2026-01-01T00:00:00Z","updatedAt":"2026-01-01T00:00:00Z","nodes":[{"id":"image","type":"image","metadata":{"history":[{"localMedia":reference}]}}],"connections":[]});
        canvas.save_human_project(project.clone()).unwrap();
        let before = canvas.get_project("film").unwrap();
        assert_eq!(read_registered_media(&canvas,directory.path(),"film","local-ref:a").unwrap(),b"original bytes");
        assert!(read_registered_media(&canvas,directory.path(),"film","local-ref:unregistered").is_err());
        assert!(read_registered_media(&canvas,directory.path(),"other-film","local-ref:a").is_err());
        fs::write(media.join("original.png"),b"tampered bytes").unwrap();
        assert!(read_registered_media(&canvas,directory.path(),"film","local-ref:a").unwrap_err().contains("校验"));
        assert_eq!(canvas.get_project("film").unwrap().revision,before.revision);
    }
}
