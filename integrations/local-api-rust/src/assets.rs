use crate::{db::ApiResult, prompts::text, query::Query};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::collections::HashSet;

pub fn list(db: &Connection, query: &Query) -> ApiResult<Value> {
    let tx = db.unchecked_transaction().map_err(|e| e.to_string())?;
    let db = &tx;
    let base = "SELECT * FROM assets WHERE (?1='' OR title LIKE ?2 OR description LIKE ?2 OR content LIKE ?2) AND (?3='' OR ?3='all' OR ?3='全部' OR type=?3)";
    let keyword = format!("%{}%", query.keyword);
    let mut facet_query = db
        .prepare(&format!(
            "SELECT tags FROM ({base}) ORDER BY updated_at DESC"
        ))
        .map_err(|e| e.to_string())?;
    let rows = facet_query
        .query_map(params![query.keyword, keyword, query.kind], |r| {
            Ok(serde_json::from_str::<Value>(&text(r, "tags")?).unwrap_or(json!([])))
        })
        .map_err(|e| e.to_string())?;
    let mut seen = HashSet::new();
    let mut tags = Vec::new();
    for row in rows {
        if let Some(values) = row.map_err(|e| e.to_string())?.as_array() {
            for tag in values.iter().filter_map(Value::as_str) {
                if !tag.is_empty() && seen.insert(tag.to_owned()) {
                    tags.push(tag.to_owned());
                }
            }
        }
    }
    let filtered = format!(
        "SELECT * FROM ({base}) WHERE {}",
        crate::query::tag_filter(false)
    );
    let selected = json!(query.tags).to_string();
    let total: i64 = db
        .query_row(
            &format!("SELECT count(*) FROM ({filtered})"),
            params![query.keyword, keyword, query.kind, selected],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    let (skip, take) = query.bounds();
    let mut statement = db
        .prepare(&format!(
            "{filtered} ORDER BY updated_at DESC LIMIT ?5 OFFSET ?6"
        ))
        .map_err(|e| e.to_string())?;
    let rows = statement
        .query_map(
            params![
                query.keyword,
                keyword,
                query.kind,
                selected,
                take as i64,
                skip.min(i64::MAX as usize) as i64
            ],
            |row| {
                let mut item = json!({});
                for (field, column) in [
                    ("id", "id"),
                    ("title", "title"),
                    ("type", "type"),
                    ("coverUrl", "cover_url"),
                    ("category", "category"),
                    ("description", "description"),
                    ("content", "content"),
                    ("url", "url"),
                    ("createdAt", "created_at"),
                    ("updatedAt", "updated_at"),
                ] {
                    item[field] = json!(text(row, column)?);
                }
                item["tags"] = serde_json::from_str(&text(row, "tags")?).unwrap_or(json!([]));
                Ok(item)
            },
        )
        .map_err(|e| e.to_string())?;
    let mut items = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    for item in &mut items {
        for key in ["url", "content"] {
            if item[key] == "" {
                item.as_object_mut().unwrap().remove(key);
            }
        }
    }
    Ok(json!({"items":items,"tags":tags,"total":total}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn expanded_assets_and_concurrent_updates_keep_snapshot_semantics() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        crate::db::initialize(&path).unwrap();
        let mut db = crate::db::connect(&path).unwrap();
        db.execute_batch("PRAGMA journal_mode=WAL;").unwrap();
        let tx = db.transaction().unwrap();
        for i in 0..20000 {
            tx.execute("INSERT INTO assets(id,title,type,tags,content,updated_at) VALUES(?1,?2,'image',?3,?4,?5)",params![format!("asset-{i:06}"),format!("中文素材 {i}"),if i%2==0 {"[\"人物\",\"中文\"]"} else {"[\"风景\"]"},"大文件引用".repeat(200),format!("{i:06}")]).unwrap();
        }
        tx.commit().unwrap();
        let q = Query::parse(Some("keyword=中文&tag=人物&tag=风景&page=2&pageSize=17"));
        let page = list(&db, &q).unwrap();
        assert_eq!(page["total"], 20000);
        assert_eq!(page["items"].as_array().unwrap().len(), 17);
        assert_eq!(page["items"][0]["id"], "asset-019982");
        assert_eq!(page["tags"], json!(["风景", "人物", "中文"]));
        assert_eq!(
            list(&db, &Query::parse(Some("page=99999999999"))).unwrap()["items"],
            json!([])
        );
        assert_eq!(
            list(&db, &Query::parse(Some("tag=missing"))).unwrap()["total"],
            0
        );
        db.execute("DELETE FROM assets WHERE id<'asset-019800'", [])
            .unwrap();
        let writer_path = path.clone();
        let writer = std::thread::spawn(move || {
            let db = crate::db::connect(&writer_path).unwrap();
            for i in 0..40 {
                db.execute(
                    "UPDATE assets SET tags=?1",
                    [if i % 2 == 0 { "[\"A\"]" } else { "[\"B\"]" }],
                )
                .unwrap();
            }
        });
        for _ in 0..40 {
            let page = list(&db, &Query::parse(Some("tag=A"))).unwrap();
            if page["total"] == 200 {
                assert_eq!(page["tags"], json!(["A"]));
                assert_eq!(page["items"][0]["tags"], json!(["A"]));
            } else {
                assert_eq!(page["total"], 0);
                assert_eq!(page["items"], json!([]));
            }
        }
        writer.join().unwrap();
    }
}
