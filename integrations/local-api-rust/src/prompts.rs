use crate::{
    db::{self, ApiResult},
    query::Query,
};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::HashSet, path::Path};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Prompt {
    pub id: String,
    pub title: String,
    pub cover_url: String,
    pub prompt: String,
    pub tags: Vec<String>,
    pub category: String,
    pub github_url: String,
    pub preview: String,
    pub created_at: String,
    pub updated_at: String,
    pub remote: bool,
    pub saved: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Category {
    pub category: String,
    pub name: String,
    pub description: String,
    pub github_url: String,
    pub source_type: String,
    pub path_or_url: String,
    pub remote: bool,
    pub enabled: bool,
    pub updated_at: String,
    pub index_updated_at: String,
}
pub fn text(row: &Row<'_>, column: &str) -> rusqlite::Result<String> {
    Ok(row.get::<_, Option<String>>(column)?.unwrap_or_default())
}
fn tags(row: &Row<'_>) -> rusqlite::Result<Vec<String>> {
    Ok(serde_json::from_str(&text(row, "tags")?).unwrap_or_default())
}
fn category_row(row: &Row<'_>) -> rusqlite::Result<Category> {
    Ok(Category {
        category: text(row, "category")?,
        name: text(row, "name")?,
        description: text(row, "description")?,
        github_url: text(row, "github_url")?,
        source_type: text(row, "source_type")?,
        path_or_url: text(row, "path_or_url")?,
        remote: row.get::<_, Option<bool>>("remote")?.unwrap_or(false),
        enabled: row.get::<_, Option<bool>>("enabled")?.unwrap_or(false),
        updated_at: text(row, "updated_at")?,
        index_updated_at: text(row, "index_updated_at")?,
    })
}
fn prompt_row(row: &Row<'_>) -> rusqlite::Result<Prompt> {
    Ok(Prompt {
        id: text(row, "id")?,
        title: text(row, "title")?,
        cover_url: text(row, "cover_url")?,
        prompt: text(row, "prompt")?,
        tags: tags(row)?,
        category: text(row, "category")?,
        github_url: text(row, "github_url")?,
        preview: text(row, "preview")?,
        created_at: text(row, "created_at")?,
        updated_at: text(row, "updated_at")?,
        ..Prompt::default()
    })
}
pub fn categories(db: &Connection) -> ApiResult<Vec<Category>> {
    let mut statement = db
        .prepare("SELECT * FROM prompt_categories ORDER BY updated_at DESC,category ASC")
        .map_err(|e| e.to_string())?;
    let rows = statement
        .query_map([], category_row)
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}
pub fn category(db: &Connection, id: &str) -> ApiResult<Category> {
    db.query_row(
        "SELECT * FROM prompt_categories WHERE category=?1",
        [id],
        category_row,
    )
    .optional()
    .map_err(|e| e.to_string())?
    .ok_or_else(|| "未知提示词分类".into())
}
pub fn save_category(db: &Connection, mut item: Category) -> ApiResult<Category> {
    if item.category.trim().is_empty() {
        return Err("分类编码不能为空".into());
    }
    if item.name.is_empty() {
        item.name = item.category.clone();
    }
    item.updated_at = db::now();
    db.execute("INSERT INTO prompt_categories(category,name,description,github_url,source_type,path_or_url,remote,enabled,updated_at,index_updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10) ON CONFLICT(category) DO UPDATE SET name=excluded.name,description=excluded.description,github_url=excluded.github_url,source_type=excluded.source_type,path_or_url=excluded.path_or_url,remote=excluded.remote,enabled=excluded.enabled,updated_at=excluded.updated_at",params![item.category,item.name,item.description,item.github_url,item.source_type,item.path_or_url,item.remote,item.enabled,item.updated_at,item.index_updated_at]).map_err(|e|e.to_string())?;
    Ok(item)
}
pub fn initialize(db: &mut Connection) -> ApiResult<()> {
    db.execute_batch("CREATE INDEX IF NOT EXISTS idx_prompt_catalogs_page ON prompt_catalogs(updated_at DESC,id ASC); CREATE INDEX IF NOT EXISTS idx_prompts_page ON prompts(updated_at DESC,id ASC); CREATE INDEX IF NOT EXISTS idx_prompt_favorites_page ON prompt_favorites(updated_at DESC,id ASC); CREATE INDEX IF NOT EXISTS idx_assets_page ON assets(updated_at DESC);").map_err(|e|e.to_string())?;
    if db
        .query_row("SELECT count(*) FROM prompt_categories", [], |r| {
            r.get::<_, i64>(0)
        })
        .map_err(|e| e.to_string())?
        == 0
    {
        let seeds: Vec<Category> =
            serde_json::from_str(include_str!("prompt-seeds.json")).map_err(|e| e.to_string())?;
        let tx = db.transaction().map_err(|e| e.to_string())?;
        for seed in seeds {
            save_category(&tx, seed)?;
        }
        tx.commit().map_err(|e| e.to_string())?;
    }
    for source in categories(db)? {
        if (!source.remote && source.source_type.is_empty()) || !source.index_updated_at.is_empty()
        {
            continue;
        }
        let items = {
            let mut statement = db
                .prepare("SELECT *,'' AS github_url FROM prompts WHERE category=?1")
                .map_err(|e| e.to_string())?;
            let rows = statement
                .query_map([&source.category], prompt_row)
                .map_err(|e| e.to_string())?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?
        };
        replace_catalog(db, &source, &items)?;
    }
    Ok(())
}
pub fn content_hash(category: &str, item: &Prompt) -> String {
    format!(
        "{:x}",
        Sha256::digest(format!("{category}\n{}\n{}", item.title, item.prompt.trim()).as_bytes())
    )
}
pub fn replace_catalog(
    db: &mut Connection,
    category: &Category,
    items: &[Prompt],
) -> ApiResult<usize> {
    let tx = db.transaction().map_err(|e| e.to_string())?;
    tx.execute(
        "DELETE FROM prompt_catalogs WHERE category=?1",
        [&category.category],
    )
    .map_err(|e| e.to_string())?;
    let mut seen = HashSet::new();
    for item in items {
        if item.prompt.trim().is_empty() || item.title.is_empty() {
            continue;
        }
        let hash = content_hash(&category.category, item);
        if !seen.insert(hash.clone()) {
            continue;
        }
        let source = if category.github_url.is_empty() {
            &category.path_or_url
        } else {
            &category.github_url
        };
        tx.execute("INSERT INTO prompt_catalogs(id,title,cover_url,tags,category,github_url,preview,content_hash,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",params![format!("catalog-{hash}"),item.title,item.cover_url,json!(item.tags).to_string(),category.category,source,item.tags.join(" · "),hash,item.created_at,item.updated_at]).map_err(|e|e.to_string())?;
    }
    tx.execute(
        "UPDATE prompt_categories SET index_updated_at=?1 WHERE category=?2",
        params![db::now(), category.category],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(seen.len())
}
const DIRECTORY:&str="SELECT id,title,cover_url,tags,category,github_url,preview,content_hash,created_at,updated_at,'' AS prompt FROM prompt_catalogs WHERE category IN (SELECT category FROM prompt_categories WHERE enabled=1) UNION ALL SELECT id,title,cover_url,tags,category,'' AS github_url,'' AS preview,'' AS content_hash,created_at,updated_at,'' AS prompt FROM prompts WHERE category NOT IN (SELECT category FROM prompt_categories WHERE remote=1 OR source_type<>'')";
pub fn list(db: &Connection, query: &Query) -> ApiResult<Value> {
    let source = if query.favorites {
        "SELECT id,title,cover_url,tags,category,source_url AS github_url,preview,'' AS content_hash,created_at,updated_at,'' AS prompt FROM prompt_favorites"
    } else {
        DIRECTORY
    };
    // All result sections observe one SQLite snapshot, including concurrent favorites/sync.
    let tx = db.unchecked_transaction().map_err(|e| e.to_string())?;
    let db = &tx;
    let base = format!("SELECT * FROM ({source}) WHERE (?1='' OR title LIKE ?2 OR category LIKE ?2 OR preview LIKE ?2 OR tags LIKE ?2) AND (?3='' OR ?3='全部' OR ?3='all' OR category=?3)");
    let keyword = format!("%{}%", query.keyword);
    // Facets precede tag filtering and preserve first occurrence in the original order.
    // Only tags are decoded here, never the entire matching prompt directory.
    let mut facets = db
        .prepare(&format!(
            "SELECT tags FROM ({base}) ORDER BY updated_at DESC,id ASC"
        ))
        .map_err(|e| e.to_string())?;
    let rows = facets
        .query_map(params![query.keyword, keyword, query.category], tags)
        .map_err(|e| e.to_string())?;
    let mut seen = HashSet::new();
    let mut tags = Vec::new();
    for row in rows {
        for tag in row.map_err(|e| e.to_string())? {
            if !tag.is_empty() && seen.insert(tag.clone()) {
                tags.push(tag);
            }
        }
    }
    let filter = crate::query::tag_filter(true);
    let selected = json!(query.tags).to_string();
    let filtered = format!("SELECT * FROM ({base}) WHERE {filter}");
    let total: i64 = db
        .query_row(
            &format!("SELECT count(*) FROM ({filtered})"),
            params![query.keyword, keyword, query.category, selected],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    let (skip, take) = query.bounds();
    let mut statement = db.prepare(&format!("SELECT *,EXISTS(SELECT 1 FROM prompt_favorites f WHERE f.id=p.id) AS is_saved FROM ({filtered}) p ORDER BY updated_at DESC,id ASC LIMIT ?5 OFFSET ?6")).map_err(|e|e.to_string())?;
    let items = statement
        .query_map(
            params![
                query.keyword,
                keyword,
                query.category,
                selected,
                take as i64,
                skip.min(i64::MAX as usize) as i64
            ],
            |r| {
                let mut p = prompt_row(r)?;
                p.remote = p.id.starts_with("catalog-");
                p.saved = r.get("is_saved")?;
                Ok(p)
            },
        )
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let mut statement = db
        .prepare(&format!(
            "SELECT DISTINCT category FROM ({source}) ORDER BY category ASC"
        ))
        .map_err(|e| e.to_string())?;
    let categories = statement
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(json!({"items":items,"tags":tags,"categories":categories,"total":total}))
}
fn stored_prompt(db: &Connection, id: &str, favorite: bool) -> ApiResult<Option<Prompt>> {
    let sql = if favorite {
        "SELECT *,source_url AS github_url FROM prompt_favorites WHERE id=?1"
    } else {
        "SELECT *,'' AS github_url FROM prompts WHERE id=?1 AND category NOT IN (SELECT category FROM prompt_categories WHERE remote=1 OR source_type<>'')"
    };
    db.query_row(sql, [id], prompt_row)
        .optional()
        .map_err(|e| e.to_string())
}
fn catalog_prompt(db: &Connection, id: &str) -> ApiResult<(Prompt, String)> {
    db.query_row(
        "SELECT *,'' AS prompt FROM prompt_catalogs WHERE id=?1",
        [id],
        |r| Ok((prompt_row(r)?, text(r, "content_hash")?)),
    )
    .optional()
    .map_err(|e| e.to_string())?
    .ok_or_else(|| "该目录条目已更新，请刷新目录后重新选择".into())
}
pub fn detail(path: &Path, id: &str) -> ApiResult<Prompt> {
    let db = db::connect(path)?;
    if let Some(mut item) = stored_prompt(&db, id, true)? {
        item.saved = true;
        item.remote = id.starts_with("catalog-");
        return Ok(item);
    }
    if !id.starts_with("catalog-") {
        return stored_prompt(&db, id, false)?.ok_or_else(|| "提示词不存在".into());
    }
    let (entry, hash) = catalog_prompt(&db, id)?;
    let source = category(&db, &entry.category)?;
    if !source.enabled {
        return Err("此订阅源当前不可用；已收藏的内容仍可离线使用".into());
    }
    drop(db);
    let items = crate::prompt_sources::load(&source)
        .map_err(|_| "加载原文失败，请检查网络或来源；目录及收藏未受影响")?;
    let mut item = items
        .into_iter()
        .find(|p| content_hash(&entry.category, p) == hash)
        .ok_or("来源内容已变更，请先更新目录后重新选择；不会使用旧序号替换成另一条提示词")?;
    item.id = entry.id;
    item.category = entry.category;
    item.github_url = entry.github_url;
    item.remote = true;
    Ok(item)
}
pub fn favorite(db: &Connection, mut item: Prompt) -> ApiResult<Value> {
    if item.prompt.is_empty() || item.prompt.len() > 1024 * 1024 {
        return Err("请先加载完整提示词再收藏".into());
    }
    if item.id.starts_with("catalog-") {
        let (entry, hash) = catalog_prompt(db, &item.id)?;
        if content_hash(&entry.category, &item) != hash {
            return Err("提示词与所选目录不一致，请重新加载".into());
        }
        item = Prompt {
            prompt: item.prompt,
            ..entry
        };
    } else {
        item = stored_prompt(db, &item.id, false)?.ok_or("提示词不存在")?;
        item.preview = item.tags.join(" · ");
    }
    db.execute("INSERT OR IGNORE INTO prompt_favorites(id,title,cover_url,prompt,tags,category,preview,created_at,updated_at,source_url,saved_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",params![item.id,item.title,item.cover_url,item.prompt,json!(item.tags).to_string(),item.category,item.preview,item.created_at,item.updated_at,item.github_url,db::now()]).map_err(|e|e.to_string())?;
    Ok(json!(true))
}
pub fn sync(path: &Path, id: &str) -> ApiResult<Vec<Category>> {
    let mut db = db::connect(path)?;
    let source = category(&db, id)?;
    if !source.enabled {
        return Err("此订阅源已停用".into());
    }
    let items = crate::prompt_sources::load(&source)?;
    if !items
        .iter()
        .any(|p| !p.title.is_empty() && !p.prompt.trim().is_empty())
    {
        return Err("来源没有解析出提示词，已保留上一次目录".into());
    }
    // Reread after network I/O: a disabled/edited source must not be replaced by a late response.
    let current = category(&db, id)?;
    if current.updated_at != source.updated_at
        || !current.enabled
        || current.path_or_url != source.path_or_url
    {
        return Err("订阅源已修改，请重新更新目录".into());
    }
    replace_catalog(&mut db, &source, &items)?;
    categories(&db)
}
pub fn sync_all(path: &Path) -> ApiResult<Value> {
    let sources = categories(&db::connect(path)?)?;
    Ok(json!(sources
        .iter()
        .filter(|s| s.enabled && (s.remote || !s.source_type.is_empty()))
        .map(|source| {
            let mut result = json!({"category":source.category,"name":source.name});
            if sync(path, &source.category).is_err() {
                result["error"] = json!("更新失败，已保留原目录");
            }
            result
        })
        .collect::<Vec<_>>()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_favorites_and_failed_source_preserve_full_text_boundary() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        db::initialize(&path).unwrap();
        let mut db = db::connect(&path).unwrap();
        let file = dir.path().join("来源.md");
        std::fs::write(
            &file,
            "## 一个镜头\n```text\nShot of a calm Chinese garden.\n```\n",
        )
        .unwrap();
        let source = save_category(
            &db,
            Category {
                category: "source".into(),
                source_type: "local_markdown".into(),
                path_or_url: file.to_string_lossy().into(),
                enabled: true,
                ..Category::default()
            },
        )
        .unwrap();
        let items = crate::prompt_sources::load(&source).unwrap();
        replace_catalog(&mut db, &source, &items).unwrap();
        let page = list(&db, &Query::parse(None)).unwrap();
        let id = page["items"][0]["id"].as_str().unwrap();
        assert_eq!(page["items"][0]["prompt"], "");
        assert!(!page.to_string().contains("Chinese garden"));
        let detail = detail(&path, id).unwrap();
        favorite(&db, detail.clone()).unwrap();
        favorite(&db, detail).unwrap();
        assert_eq!(
            db.query_row("SELECT count(*) FROM prompt_favorites", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
        std::fs::remove_file(file).unwrap();
        assert!(sync(&path, "source").is_err());
        assert!(super::detail(&path, id)
            .unwrap()
            .prompt
            .contains("Chinese garden"));
        assert_eq!(list(&db, &Query::parse(None)).unwrap()["total"], 1);
    }
}

#[cfg(test)]
mod pagination_tests {
    use super::*;
    // Frozen pre-optimization list algorithm, used as the compatibility oracle.
    fn legacy(db: &Connection, query: &Query) -> Value {
        let source = if query.favorites {
            "SELECT id,title,cover_url,tags,category,source_url AS github_url,preview,'' AS content_hash,created_at,updated_at,'' AS prompt FROM prompt_favorites"
        } else {
            DIRECTORY
        };
        let mut statement=db.prepare(&format!("SELECT * FROM ({source}) WHERE (?1='' OR title LIKE ?2 OR category LIKE ?2 OR preview LIKE ?2 OR tags LIKE ?2) AND (?3='' OR ?3='全部' OR ?3='all' OR category=?3) ORDER BY updated_at DESC,id ASC")).unwrap();
        let mut items = statement
            .query_map(
                params![
                    query.keyword,
                    format!("%{}%", query.keyword),
                    query.category
                ],
                prompt_row,
            )
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let mut seen = HashSet::new();
        let tags = items
            .iter()
            .flat_map(|p| p.tags.iter())
            .filter(|t| !t.is_empty() && seen.insert((*t).clone()))
            .cloned()
            .collect::<Vec<_>>();
        items.retain(|p| query.matches_tags(&p.tags));
        let total = items.len();
        let (skip, take) = query.bounds();
        let items = items
            .into_iter()
            .skip(skip)
            .take(take)
            .map(|mut p| {
                p.remote = p.id.starts_with("catalog-");
                p.saved = db
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM prompt_favorites WHERE id=?1)",
                        [&p.id],
                        |r| r.get(0),
                    )
                    .unwrap();
                p
            })
            .collect::<Vec<_>>();
        let mut statement = db
            .prepare(&format!(
                "SELECT DISTINCT category FROM ({source}) ORDER BY category ASC"
            ))
            .unwrap();
        let categories = statement
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        json!({"items":items,"tags":tags,"categories":categories,"total":total})
    }
    #[test]
    fn pagination_matches_legacy_real_copy_and_expanded_directory() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("copy.db");
        if let Ok(source) = std::env::var("CANVAS_TEST_READONLY_DB") {
            let source =
                Connection::open_with_flags(source, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                    .unwrap();
            source
                .backup(rusqlite::DatabaseName::Main, &path, None)
                .unwrap();
        }
        db::initialize(&path).unwrap();
        let mut db = db::connect(&path).unwrap();
        for raw in [
            "",
            "keyword=中国",
            "tag=摄影&tag=人物",
            "page=99999",
            "favorites=true",
            "page=0&pageSize=0",
            "keyword=%25",
        ] {
            let q = Query::parse(Some(raw));
            assert_eq!(list(&db, &q).unwrap(), legacy(&db, &q), "real {raw}");
        }
        let tx = db.transaction().unwrap();
        for i in 0..20000 {
            let tag = if i % 7 == 0 {
                "invalid"
            } else if i % 11 == 0 {
                "[\"人物\",3]"
            } else {
                "[\"人物\",\"中文\",\"\"]"
            };
            tx.execute("INSERT INTO prompts(id,title,tags,category,updated_at) VALUES(?1,?2,?3,'fixture',?4)",params![format!("fixture-{i:06}"),format!("中文人物 {i}"),tag,format!("{:06}",i%50)]).unwrap();
        }
        tx.execute("INSERT INTO prompt_favorites(id,title,tags,category) VALUES('fixture-000001','收藏','[\"人物\"]','fixture')",[]).unwrap();
        tx.commit().unwrap();
        for raw in [
            "",
            "tag=人物&tag=missing",
            "tag=missing",
            "tag=",
            "keyword=中文&page=2&pageSize=17",
            "category=fixture",
            "favorites=true",
            "page=18446744073709551615",
            "pageSize=500",
        ] {
            let q = Query::parse(Some(raw));
            assert_eq!(list(&db, &q).unwrap(), legacy(&db, &q), "expanded {raw}");
        }
        let q = Query::parse(None);
        let start = std::time::Instant::now();
        for _ in 0..5 {
            legacy(&db, &q);
        }
        let old = start.elapsed();
        let start = std::time::Instant::now();
        for _ in 0..5 {
            list(&db, &q).unwrap();
        }
        let new = start.elapsed();
        eprintln!("pagination benchmark debug SQLite 20000 added rows, 5 runs: old={old:?}, new={new:?}; full row materialization 20000+ -> 20 (facets still scan tags)");
    }
}
