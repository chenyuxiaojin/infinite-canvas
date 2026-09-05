use local_agent_adapter::{CanvasOperationAdapter, SqliteCanvasAdapter};
use rusqlite::Connection;
use serde_json::Value;

// External JSON decimals; expected bits come from Rust's compiler, NOT serde_json.
const SOURCE: &str = r#"{"id":"float-project","title":"precision fixture","createdAt":"2026-01-01T00:00:00Z","updatedAt":"2026-01-01T00:00:00Z","viewport":{"x":-222.56302150354014,"y":222.56302150354014,"k":0.10000000000000002},"nodes":[{"id":"node-1","type":"text","position":{"x":222.56302150354014,"y":-0.10000000000000002}}],"connections":[],"chatSessions":[],"unknownCompatibleFloat":1.0000000000000002}"#;
fn assert_coordinate_bits(project: &Value) {
    for (pointer, expected) in [
        ("/viewport/x", -222.56302150354014_f64),
        ("/viewport/y", 222.56302150354014_f64),
        ("/viewport/k", 0.10000000000000002_f64),
        ("/nodes/0/position/x", 222.56302150354014_f64),
        ("/nodes/0/position/y", -0.10000000000000002_f64),
        ("/unknownCompatibleFloat", 1.0000000000000002_f64),
    ] {
        assert_eq!(
            project
                .pointer(pointer)
                .unwrap()
                .as_f64()
                .unwrap()
                .to_bits(),
            expected.to_bits(),
            "coordinate changed: {pointer}"
        );
    }
}
#[test]
fn actual_save_get_and_reopen_preserve_external_coordinate_bits() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("precision.db");
    Connection::open(&path).unwrap().execute_batch("CREATE TABLE canvas_projects(user_id TEXT,id TEXT,project_data TEXT,created_at TEXT,updated_at TEXT,deleted_at TEXT NOT NULL DEFAULT '',PRIMARY KEY(user_id,id));").unwrap();
    let adapter = SqliteCanvasAdapter::open(&path).unwrap();
    let project: Value = serde_json::from_str(SOURCE).unwrap();
    assert_coordinate_bits(&project);
    assert_coordinate_bits(
        &adapter
            .save_human_project_checked(project, Some(""))
            .unwrap(),
    );
    for round in 1..=20 {
        // Reopen the actual adapter/database each time, including IPC-like JSON serialization.
        let reopened = SqliteCanvasAdapter::open(&path).unwrap();
        let document = reopened.get_project("float-project").unwrap();
        assert_coordinate_bits(&document.project);
        let external = serde_json::to_string(&document.project).unwrap();
        let mut incoming: Value = serde_json::from_str(&external).unwrap();
        assert_coordinate_bits(&incoming);
        incoming["updatedAt"] = Value::String(format!("2026-01-01T00:00:{round:02}Z"));
        let saved = reopened
            .save_human_project_checked(incoming, Some(&document.revision))
            .unwrap();
        assert_coordinate_bits(&saved);
        let confirmation = reopened.get_project("float-project").unwrap();
        assert_coordinate_bits(&confirmation.project);
        assert_eq!(
            saved, confirmation.project,
            "save confirmation changed coordinates"
        );
    }
    let versions = adapter.history_list("float-project").unwrap();
    for version in versions.as_array().unwrap() {
        let preview = adapter
            .history_preview("float-project", version["sequence"].as_i64().unwrap())
            .unwrap();
        assert_coordinate_bits(&preview["project"]);
    }
}
