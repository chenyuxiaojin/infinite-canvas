use crate::db::ApiResult;
use rusqlite::{Connection, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashSet;

pub fn read(db: &Connection, key: &str) -> ApiResult<Value> {
    let value = db
        .query_row("SELECT value FROM settings WHERE key=?1", [key], |r| {
            r.get::<_, String>(0)
        })
        .optional()
        .map_err(|e| e.to_string())?;
    match value {
        Some(raw) => {
            serde_json::from_str(&raw).map_err(|_| "本机配置格式无效，请从备份恢复".into())
        }
        None => Ok(json!({})),
    }
}
fn text<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or_default()
}
fn unique(values: impl Iterator<Item = String>) -> Vec<String> {
    let mut seen = HashSet::new();
    values
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty() && seen.insert(v.clone()))
        .collect()
}
pub fn public(db: &Connection) -> ApiResult<Value> {
    let saved = read(db, "public")?;
    let private = read(db, "private")?;
    let config = &saved["modelChannel"];
    let channels = private["channels"].as_array().cloned().unwrap_or_default();
    let enabled_models = unique(
        channels
            .iter()
            .filter(|c| c["enabled"] == true)
            .flat_map(|c| c["models"].as_array().cloned().unwrap_or_default())
            .filter_map(|m| m.as_str().map(str::to_owned)),
    );
    let selected = unique(
        config["availableModels"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|m| m.as_str().map(str::to_owned)),
    )
    .into_iter()
    .filter(|m| enabled_models.contains(m))
    .collect::<Vec<_>>();
    let mut info = Vec::new();
    for channel in channels.iter().filter(|c| {
        c["enabled"] == true
            && !text(c, "baseUrl").is_empty()
            && c["models"].as_array().is_some_and(|ms| !ms.is_empty())
    }) {
        let id = if text(channel, "id").is_empty() {
            format!(
                "channel-{}",
                &format!(
                    "{:x}",
                    Sha256::digest(
                        format!("{}|{}", text(channel, "name"), text(channel, "baseUrl"))
                            .as_bytes()
                    )
                )[..16]
            )
        } else {
            text(channel, "id").into()
        };
        info.push(json!({"id":id,"protocol":if text(channel,"protocol").is_empty(){"openai"}else{text(channel,"protocol")},"name":text(channel,"name"),"baseUrl":text(channel,"baseUrl"),"models":channel["models"],"weight":channel["weight"].as_i64().unwrap_or(1).max(1),"timeout":channel["timeout"].as_i64().filter(|n|*n>0).unwrap_or(600),"enabled":true,"remark":text(channel,"remark")}));
    }
    let mut system = json!({});
    for kind in ["image", "video", "text", "workflow", "workflowAgent"] {
        let raw = text(&config["systemPrompts"], kind);
        system[kind] = json!(if raw.trim().is_empty() {
            match kind {
                "workflowAgent" => include_str!("workflow-agent-prompt.txt"),
                "image" | "text" => text(config, "systemPrompt"),
                _ => "",
            }
        } else {
            raw
        });
    }
    let costs=config["modelCosts"].as_array().cloned().unwrap_or_default().iter().map(|item|json!({"model":text(item,"model").trim(),"credits":item["credits"].as_i64().unwrap_or_default().max(0)})).collect::<Vec<_>>();
    let mut channel = json!({"availableModels":if selected.is_empty(){&enabled_models}else{&selected},"modelCosts":costs,"channels":info,"systemPrompt":text(config,"systemPrompt"),"systemPrompts":system,"allowCustomChannel":config["allowCustomChannel"].as_bool().unwrap_or(true),"allowUserRemoteChannel":config["allowUserRemoteChannel"].as_bool().unwrap_or(false)});
    for (key, kind) in [
        ("defaultModel", "text"),
        ("defaultTextModel", "text"),
        ("defaultImageModel", "image"),
        ("defaultVideoModel", "video"),
    ] {
        let current = text(config, key).trim();
        let fallback = selected
            .iter()
            .find(|m| model_kind(m) == kind)
            .or_else(|| selected.first());
        channel[key] = json!(if selected.iter().any(|m| m == current) {
            current
        } else {
            fallback.map(String::as_str).unwrap_or_default()
        });
    }
    Ok(
        json!({"modelChannel":channel,"auth":{"allowRegister":saved["auth"]["allowRegister"].as_bool().unwrap_or(true),"linuxDo":{"enabled":saved["auth"]["linuxDo"]["enabled"].as_bool().unwrap_or(false)}},"storage":{"mode":text(&saved["storage"],"mode"),"allowUserProvider":saved["storage"]["allowUserProvider"].as_bool().unwrap_or(false),"allowUserGlobalProvider":saved["storage"]["allowUserGlobalProvider"].as_bool().unwrap_or(false)}}),
    )
}
fn model_kind(model: &str) -> &str {
    let name = model.trim().to_lowercase();
    if name == "minimax-h3" || name.contains("seedance") || name.contains("video") {
        "video"
    } else if name.contains("seedream") || name.contains("image") {
        "image"
    } else {
        "text"
    }
}
pub fn storage(db: &Connection) -> ApiResult<Value> {
    let private = read(db, "private")?;
    let s = &private["storage"];
    let providers = s["providers"].as_array().cloned().unwrap_or_default();
    let global = providers
        .iter()
        .any(|p| p["enabled"] == true && text(p, "ownerUserId").is_empty());
    let user = s["allowUserProvider"].as_bool().unwrap_or(false);
    Ok(
        json!({"mode":if global{"server_sqlite_s3"}else if user{"hybrid"}else{"local_indexeddb"},"allowUserProvider":user,"allowUserGlobalProvider":if text(s,"mode").is_empty(){true}else{s["allowUserGlobalProvider"].as_bool().unwrap_or(false)}}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn public_settings_never_return_private_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.db");
        crate::db::initialize(&path).unwrap();
        let db = crate::db::connect(&path).unwrap();
        db.execute("INSERT INTO settings(key,value) VALUES('private',?1)",[json!({"channels":[{"name":"test","baseUrl":"https://example.com","apiKey":"secret-never-public","enabled":true,"models":["test-model"]}],"storage":{"providers":[]}}).to_string()]).unwrap();
        let result = public(&db).unwrap();
        assert!(!result.to_string().contains("secret-never-public"));
        assert_eq!(
            result["modelChannel"]["availableModels"],
            json!(["test-model"])
        );
        assert_eq!(result["modelChannel"]["defaultModel"], "");
        assert_eq!(storage(&db).unwrap()["mode"], "local_indexeddb");
    }
}
