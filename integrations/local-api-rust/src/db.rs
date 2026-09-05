use rusqlite::{params, Connection, TransactionBehavior};
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

pub type ApiResult<T> = Result<T, String>;

pub fn connect(path: &Path) -> ApiResult<Connection> {
    let db = Connection::open(path).map_err(|e| e.to_string())?;
    db.busy_timeout(Duration::from_secs(10))
        .map_err(|e| e.to_string())?;
    db.execute_batch("PRAGMA foreign_keys=ON;")
        .map_err(|e| e.to_string())?;
    Ok(db)
}

pub fn now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .expect("UTC timestamp")
}

pub fn initialize(path: &Path) -> ApiResult<Option<PathBuf>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let existed = path.is_file();
    let mut db = connect(path)?;
    let version_table: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='desktop_schema_migrations')", [], |row| row.get(0)).map_err(|e| e.to_string())?;
    let applied = version_table
        && db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM desktop_schema_migrations WHERE version=1)",
                [],
                |row| row.get::<_, bool>(0),
            )
            .map_err(|e| e.to_string())?;
    if applied {
        return Ok(None);
    }
    let backup = if existed {
        let directory = path
            .parent()
            .unwrap_or(Path::new("."))
            .join("rust-migration-backups");
        fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
                .map_err(|e| e.to_string())?;
        }
        let target = directory.join(format!(
            "before-rust-api-{}.db",
            time::OffsetDateTime::now_utc().unix_timestamp_nanos()
        ));
        db.backup(rusqlite::DatabaseName::Main, &target, None)
            .map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&target, fs::Permissions::from_mode(0o600))
                .map_err(|e| e.to_string())?;
        }
        Some(target)
    } else {
        None
    };
    let transaction = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    transaction
        .execute_batch(include_str!("schema.sql"))
        .map_err(|e| e.to_string())?;
    transaction.execute_batch("CREATE TABLE IF NOT EXISTS desktop_schema_migrations (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL, backup_path TEXT);").map_err(|e| e.to_string())?;
    // Preserve dangling records inside the same project; never invent the missing nodes.
    let rows = {
        let mut statement = transaction
            .prepare("SELECT user_id,id,project_data FROM canvas_projects WHERE deleted_at=''")
            .map_err(|e| e.to_string())?;
        let result = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(|e| e.to_string())?;
        result
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
    };
    for (owner, id, raw) in rows {
        let mut project: Value = serde_json::from_str(&raw)
            .map_err(|_| format!("画布 {id} 的 JSON 无效，迁移已停止"))?;
        if quarantine_connections(&mut project)? > 0 {
            transaction
                .execute(
                    "UPDATE canvas_projects SET project_data=?1 WHERE user_id=?2 AND id=?3",
                    params![project.to_string(), owner, id],
                )
                .map_err(|e| e.to_string())?;
        }
    }
    transaction
        .execute(
            "INSERT INTO desktop_schema_migrations(version,applied_at,backup_path) VALUES(1,?1,?2)",
            params![
                now(),
                backup.as_ref().map(|p| p.to_string_lossy().to_string())
            ],
        )
        .map_err(|e| e.to_string())?;
    transaction.commit().map_err(|e| e.to_string())?;
    if db
        .query_row("PRAGMA quick_check", [], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        != "ok"
    {
        return Err("迁移后的数据库完整性检查失败".into());
    }
    Ok(backup)
}

pub fn quarantine_connections(project: &mut Value) -> ApiResult<usize> {
    let nodes = project["nodes"].as_array().ok_or("节点清单无效")?;
    let ids: HashSet<String> = nodes
        .iter()
        .filter_map(|node| node["id"].as_str().map(str::to_owned))
        .collect();
    if ids.len() != nodes.len() {
        return Err("节点 ID 缺失或重复，已保留原项目并停止迁移".into());
    }
    let connections = project["connections"].as_array().ok_or("关系清单无效")?;
    let mut valid = Vec::new();
    let mut invalid = Vec::new();
    for connection in connections {
        if connection["fromNodeId"]
            .as_str()
            .is_some_and(|id| ids.contains(id))
            && connection["toNodeId"]
                .as_str()
                .is_some_and(|id| ids.contains(id))
        {
            valid.push(connection.clone());
        } else {
            invalid.push(json!({"connection":connection,"reason":"关系端点不在当前节点清单；迁移时保留原记录"}));
        }
    }
    let count = invalid.len();
    if count > 0 {
        let mut saved = project
            .get("quarantinedConnections")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        saved.extend(invalid);
        project["quarantinedConnections"] = json!(saved);
        project["connections"] = json!(valid);
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quarantine_preserves_original_records_and_is_idempotent() {
        let edge = json!({"id":"edge","fromNodeId":"a","toNodeId":"missing","extra":{"keep":true}});
        let mut project = json!({"nodes":[{"id":"a"}],"connections":[edge.clone()],"unknown":"keep","updatedAt":"unchanged"});
        assert_eq!(quarantine_connections(&mut project).unwrap(), 1);
        assert_eq!(project["quarantinedConnections"][0]["connection"], edge);
        assert_eq!(project["updatedAt"], "unchanged");
        assert_eq!(project["unknown"], "keep");
        assert_eq!(quarantine_connections(&mut project).unwrap(), 0);
    }
    #[test]
    fn fresh_schema_and_repeat_start_are_safe() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("canvas.db");
        assert!(initialize(&path).unwrap().is_none());
        assert!(initialize(&path).unwrap().is_none());
        let db = connect(&path).unwrap();
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM desktop_schema_migrations",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
    }
}
