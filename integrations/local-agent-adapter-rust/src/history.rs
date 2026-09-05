//! Durable project history. Call record inside the same transaction as a successful save.
//! Only this module's records are pruned; referenced media and recovery backups are untouched.
use crate::BridgeError;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const MAX_VERSIONS: i64 = 100;
const MAX_BYTES: i64 = 64 * 1024 * 1024;
pub fn initialize(db: &Connection) -> Result<(), BridgeError> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS canvas_version_history (
        sequence INTEGER PRIMARY KEY AUTOINCREMENT, owner TEXT NOT NULL, project_id TEXT NOT NULL,
        revision TEXT NOT NULL, snapshot TEXT NOT NULL, created_at TEXT NOT NULL,
        reason TEXT NOT NULL, restored_from INTEGER, bytes INTEGER NOT NULL);
        CREATE INDEX IF NOT EXISTS idx_canvas_versions_project ON canvas_version_history(owner,project_id,sequence DESC);
        CREATE TABLE IF NOT EXISTS canvas_version_restores (
        request_id TEXT PRIMARY KEY, owner TEXT NOT NULL, project_id TEXT NOT NULL,
        source_sequence INTEGER NOT NULL, base_revision TEXT NOT NULL, result_revision TEXT NOT NULL);")?;
    Ok(())
}
fn hash(raw: &str) -> String {
    format!("{:x}", Sha256::digest(raw.as_bytes()))
}
fn now() -> Result<String, BridgeError> {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(|_| BridgeError::internal("版本时间无效"))
}
pub fn record(
    db: &Connection,
    owner: &str,
    id: &str,
    raw: &str,
    reason: &str,
    restored_from: Option<i64>,
) -> Result<(), BridgeError> {
    let revision = hash(raw);
    let latest: Option<(String,String)> = db.query_row("SELECT revision,created_at FROM canvas_version_history WHERE owner=?1 AND project_id=?2 ORDER BY sequence DESC LIMIT 1",params![owner,id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
    if let Some((previous, at)) = latest {
        if previous == revision {
            return Ok(());
        }
        if reason == "save"
            && time::OffsetDateTime::parse(&at, &time::format_description::well_known::Rfc3339)
                .is_ok_and(|at| (time::OffsetDateTime::now_utc() - at).whole_seconds() < 30)
        {
            return Ok(());
        }
    }
    db.execute("INSERT INTO canvas_version_history(owner,project_id,revision,snapshot,created_at,reason,restored_from,bytes) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",params![owner,id,revision,raw,now()?,reason,restored_from,raw.len() as i64])?;
    // Retain at least the most recent two snapshots (pre-restore and restored), even if oversized.
    db.execute("DELETE FROM canvas_version_history WHERE sequence IN (SELECT sequence FROM (SELECT sequence,row_number() OVER(ORDER BY sequence DESC) AS n,sum(bytes) OVER(ORDER BY sequence DESC) AS size FROM canvas_version_history WHERE owner=?1 AND project_id=?2) WHERE n>2 AND (n>?3 OR size>?4))",params![owner,id,MAX_VERSIONS,MAX_BYTES])?;
    Ok(())
}
fn current(db: &Connection, owner: &str, id: &str) -> Result<String, BridgeError> {
    db.query_row(
        "SELECT project_data FROM canvas_projects WHERE user_id=?1 AND id=?2 AND deleted_at=''",
        params![owner, id],
        |r| r.get(0),
    )
    .optional()?
    .ok_or_else(|| BridgeError::not_found("项目不存在或已删除，不能恢复版本"))
}
pub fn list(db: &Connection, owner: &str, id: &str) -> Result<Value, BridgeError> {
    current(db, owner, id)?;
    let mut stmt=db.prepare("SELECT sequence,revision,created_at,reason,restored_from,bytes FROM canvas_version_history WHERE owner=?1 AND project_id=?2 ORDER BY sequence DESC")?;
    let rows=stmt.query_map(params![owner,id],|r|Ok(json!({"sequence":r.get::<_,i64>(0)?,"revision":r.get::<_,String>(1)?,"createdAt":r.get::<_,String>(2)?,"reason":r.get::<_,String>(3)?,"restoredFrom":r.get::<_,Option<i64>>(4)?,"bytes":r.get::<_,i64>(5)?})))?;
    Ok(json!(rows.collect::<Result<Vec<_>, _>>()?))
}
fn snapshot(db: &Connection, owner: &str, id: &str, sequence: i64) -> Result<Value, BridgeError> {
    let raw:String=db.query_row("SELECT snapshot FROM canvas_version_history WHERE owner=?1 AND project_id=?2 AND sequence=?3",params![owner,id,sequence],|r|r.get(0)).optional()?.ok_or_else(||BridgeError::not_found("版本不存在或已超过保留范围"))?;
    serde_json::from_str(&raw).map_err(|_| BridgeError::internal("版本内容损坏，未修改当前项目"))
}
pub fn preview(
    db: &Connection,
    owner: &str,
    id: &str,
    sequence: i64,
) -> Result<Value, BridgeError> {
    let raw = current(db, owner, id)?;
    let current: Value =
        serde_json::from_str(&raw).map_err(|_| BridgeError::internal("项目内容损坏"))?;
    let target = snapshot(db, owner, id, sequence)?;
    let mut changes = Vec::new();
    for (key, label) in [
        ("nodes", "节点"),
        ("connections", "连线"),
        ("chatSessions", "对话"),
    ] {
        let empty = Vec::new();
        let before = current[key].as_array().unwrap_or(&empty);
        let after = target[key].as_array().unwrap_or(&empty);
        let added = after
            .iter()
            .filter(|n| !before.iter().any(|v| v["id"] == n["id"]))
            .count();
        let removed = before
            .iter()
            .filter(|n| !after.iter().any(|v| v["id"] == n["id"]))
            .count();
        let changed = after
            .iter()
            .filter(|n| before.iter().any(|v| v["id"] == n["id"] && v != *n))
            .count();
        changes.push(
            json!({"field":key,"label":label,"added":added,"removed":removed,"changed":changed}),
        );
    }
    Ok(json!({"sequence":sequence,"baseRevision":hash(&raw),"changes":changes,"project":target}))
}
/// Caller owns an IMMEDIATE transaction, validates the resulting project and commits it.
/// No file operations, model calls or task resubmission happen here.
pub fn restore(
    db: &Connection,
    owner: &str,
    id: &str,
    sequence: i64,
    expected: &str,
    request_id: &str,
    validate: impl FnOnce(&Value) -> Result<(), BridgeError>,
) -> Result<Value, BridgeError> {
    if request_id.is_empty() || request_id.len() > 128 {
        return Err(BridgeError::invalid("恢复请求编号无效"));
    }
    let raw = current(db, owner, id)?;
    let duplicate:Option<(String,String,i64,String,String)>=db.query_row("SELECT owner,project_id,source_sequence,base_revision,result_revision FROM canvas_version_restores WHERE request_id=?1",[request_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?;
    if let Some((o, p, s, b, result)) = duplicate {
        if (o.as_str(), p.as_str(), s, b.as_str()) != (owner, id, sequence, expected) {
            return Err(BridgeError::conflict(
                "REQUEST_CONFLICT",
                "恢复请求编号已用于其他操作",
            ));
        }
        return Ok(json!({"revision":result,"duplicate":true}));
    }
    if hash(&raw) != expected {
        return Err(BridgeError::conflict(
            "REVISION_CONFLICT",
            "画布已有新修改，请重新预览后恢复",
        ));
    }
    let mut target = snapshot(db, owner, id, sequence)?;
    let live: Value =
        serde_json::from_str(&raw).map_err(|_| BridgeError::internal("项目内容损坏"))?;
    if target["id"] != id {
        return Err(BridgeError::invalid("版本不属于当前项目"));
    }
    // Keep live binding/config identity. Historical commands must not auto-run again.
    for key in [
        "projectBinding",
        "projectDir",
        "workspaceDir",
        "binding",
        "createdAt",
    ] {
        if let Some(v) = live.get(key) {
            target[key] = v.clone();
        } else if let Some(o) = target.as_object_mut() {
            o.remove(key);
        }
    }
    target
        .as_object_mut()
        .ok_or_else(|| BridgeError::invalid("版本格式无效"))?
        .remove("pendingAgentRequest");
    if let Some(nodes) = target["nodes"].as_array_mut() {
        for node in nodes {
            if node["metadata"]["status"] == "loading" {
                node["metadata"]["status"] = json!("error");
                node["metadata"]["errorDetails"] =
                    json!("已恢复历史内容；原任务状态需核对，不会自动重新提交");
            }
        }
    }
    if let Some(sessions) = target["chatSessions"].as_array_mut() {
        for session in sessions {
            if let Some(state) = session.get_mut("agentState").and_then(Value::as_object_mut) {
                state.insert("pendingTaskIds".into(), json!([]));
            }
            if let Some(messages) = session["messages"].as_array_mut() {
                for msg in messages {
                    if matches!(
                        msg["status"].as_str(),
                        Some("thinking" | "running" | "waiting")
                    ) {
                        msg["status"] = json!("error");
                    }
                }
            }
        }
    }
    target["updatedAt"] = json!(now()?);
    target["versionRestore"] = json!({"sourceSequence":sequence,"requestId":request_id});
    validate(&target)?;
    let restored = target.to_string();
    record(db, owner, id, &raw, "before_restore", None)?;
    db.execute("UPDATE canvas_projects SET project_data=?1,updated_at=?2 WHERE user_id=?3 AND id=?4 AND project_data=?5 AND deleted_at=''",params![restored,target["updatedAt"].as_str(),owner,id,raw])?;
    record(db, owner, id, &restored, "restore", Some(sequence))?;
    let revision = hash(&restored);
    db.execute(
        "INSERT INTO canvas_version_restores VALUES(?1,?2,?3,?4,?5,?6)",
        params![request_id, owner, id, sequence, expected, revision],
    )?;
    db.execute("DELETE FROM canvas_version_restores WHERE rowid IN (SELECT rowid FROM canvas_version_restores WHERE owner=?1 AND project_id=?2 ORDER BY rowid DESC LIMIT -1 OFFSET 1000)",params![owner,id])?;
    Ok(json!({"project":target,"revision":revision,"duplicate":false}))
}

pub fn record_save(
    db: &Connection,
    owner: &str,
    id: &str,
    before: Option<&str>,
    after: &str,
) -> Result<(), BridgeError> {
    let exists: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM canvas_version_history WHERE owner=?1 AND project_id=?2)",
        params![owner, id],
        |r| r.get(0),
    )?;
    if !exists {
        if let Some(raw) = before {
            record(db, owner, id, raw, "initial", None)?;
        }
    }
    record(
        db,
        owner,
        id,
        after,
        if exists { "save" } else { "initial_save" },
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn setup(path: &std::path::Path) -> Connection {
        let db = Connection::open(path).unwrap();
        db.execute_batch("CREATE TABLE IF NOT EXISTS canvas_projects(user_id TEXT,id TEXT,project_data TEXT,updated_at TEXT,deleted_at TEXT)").unwrap();
        initialize(&db).unwrap();
        db
    }
    #[test]
    fn restart_conflict_failure_deleted_and_repeated_restores() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let mut db = setup(&path);
        let original=json!({"id":"p","nodes":[{"id":"n","metadata":{"status":"loading","storageKey":"missing-but-preserved","imageTaskId":"remote-task"}}],"connections":[],"unknown":{"keep":true},"pendingAgentRequest":{"id":"old"}}).to_string();
        db.execute(
            "INSERT INTO canvas_projects VALUES('o','p',?1,'','')",
            [&original],
        )
        .unwrap();
        record(&db, "o", "p", &original, "initial", None).unwrap();
        drop(db);
        db = setup(&path);
        assert_eq!(list(&db, "o", "p").unwrap().as_array().unwrap().len(), 1);
        let seq = list(&db, "o", "p").unwrap()[0]["sequence"]
            .as_i64()
            .unwrap();
        let edited = json!({"id":"p","nodes":[],"connections":[],"unknown":"new"}).to_string();
        db.execute("UPDATE canvas_projects SET project_data=?1", [&edited])
            .unwrap();
        let tx = db.transaction().unwrap();
        assert_eq!(
            restore(&tx, "o", "p", seq, &hash(&original), "conflict", |_| Ok(()))
                .unwrap_err()
                .code,
            "REVISION_CONFLICT"
        );
        assert!(
            restore(&tx, "o", "p", seq, &hash(&edited), "invalid", |_| Err(
                BridgeError::invalid("fixture")
            ))
            .is_err()
        );
        tx.rollback().unwrap();
        assert_eq!(current(&db, "o", "p").unwrap(), edited);
        let tx = db.transaction().unwrap();
        let result = restore(&tx, "o", "p", seq, &hash(&edited), "once", |_| Ok(())).unwrap();
        tx.commit().unwrap();
        assert_eq!(result["project"]["unknown"]["keep"], true);
        assert_eq!(
            result["project"]["nodes"][0]["metadata"]["imageTaskId"],
            "remote-task"
        );
        assert_eq!(result["project"]["nodes"][0]["metadata"]["status"], "error");
        assert!(result["project"].get("pendingAgentRequest").is_none());
        assert_eq!(
            restore(&db, "o", "p", seq, &hash(&edited), "once", |_| Ok(())).unwrap()["duplicate"],
            true
        );
        assert!(list(&db, "o", "p").unwrap().as_array().unwrap().len() >= 3);
        let revision = result["revision"].as_str().unwrap();
        let tx = db.transaction().unwrap();
        restore(&tx, "o", "p", seq, revision, "twice", |_| Ok(())).unwrap();
        tx.commit().unwrap();
        db.execute("UPDATE canvas_projects SET deleted_at='deleted'", [])
            .unwrap();
        assert!(restore(&db, "o", "p", seq, revision, "deleted", |_| Ok(())).is_err());
        assert!(preview(&db, "other", "p", seq).is_err());
    }
}

#[cfg(test)]
mod retention_tests {
    use super::*;
    #[test]
    fn failed_history_write_rolls_back_project_and_retention_only_prunes_versions() {
        let mut db = Connection::open_in_memory().unwrap();
        initialize(&db).unwrap();
        db.execute_batch("CREATE TABLE canvas_projects(user_id TEXT,id TEXT,project_data TEXT,updated_at TEXT,deleted_at TEXT); CREATE TABLE original_media(id TEXT); INSERT INTO original_media VALUES('untouched');").unwrap();
        let original = json!({"id":"p","nodes":[],"connections":[]}).to_string();
        db.execute(
            "INSERT INTO canvas_projects VALUES('o','p',?1,'','')",
            [&original],
        )
        .unwrap();
        record(&db, "o", "p", &original, "initial", None).unwrap();
        let edited = json!({"id":"p","nodes":[],"connections":[],"title":"new"}).to_string();
        db.execute("UPDATE canvas_projects SET project_data=?1", [&edited])
            .unwrap();
        db.execute_batch("CREATE TRIGGER history_disk_failure BEFORE INSERT ON canvas_version_history WHEN NEW.reason='restore' BEGIN SELECT RAISE(FAIL,'injected storage failure'); END;").unwrap();
        {
            let tx = db.transaction().unwrap();
            assert!(restore(&tx, "o", "p", 1, &hash(&edited), "fail", |_| Ok(())).is_err());
        }
        assert_eq!(current(&db, "o", "p").unwrap(), edited);
        assert_eq!(list(&db, "o", "p").unwrap().as_array().unwrap().len(), 1);
        db.execute_batch("DROP TRIGGER history_disk_failure;")
            .unwrap();
        for i in 0..120 {
            record(
                &db,
                "o",
                "p",
                &json!({"id":"p","i":i}).to_string(),
                "fixture",
                None,
            )
            .unwrap();
        }
        assert_eq!(list(&db, "o", "p").unwrap().as_array().unwrap().len(), 100);
        assert_eq!(
            db.query_row("SELECT id FROM original_media", [], |r| r
                .get::<_, String>(0))
                .unwrap(),
            "untouched"
        );
        assert_eq!(current(&db, "o", "p").unwrap(), edited);
    }
}
