//! Pure request preparation. This module never uploads files or calls a model.
use crate::db::ApiResult;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
pub type Object = Map<String, Value>;
pub fn get<'a>(object: &'a Object, key: &str) -> &'a Value {
    object.get(key).unwrap_or(&Value::Null)
}
pub fn text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        _ => value.to_string(),
    }
}
pub fn empty(value: &Value) -> bool {
    value.is_null()
        || value.as_str().is_some_and(|s| s.trim().is_empty())
        || value.as_array().is_some_and(Vec::is_empty)
}
pub fn bool_like(value: &Value) -> bool {
    match value {
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().unwrap_or_default() != 0.0,
        Value::String(s) => matches!(
            s.trim().to_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        _ => false,
    }
}
pub fn strings(value: &Value) -> Vec<String> {
    match value {
        Value::Array(xs) => xs.iter().flat_map(strings).collect(),
        _ => {
            let s = text(value).trim().to_owned();
            if s.is_empty() {
                vec![]
            } else {
                vec![s]
            }
        }
    }
}
pub fn remove(object: &mut Object, keys: &[&str]) {
    for key in keys {
        object.remove(*key);
    }
}
pub fn insert(object: &mut Object, key: &str, value: impl Into<Value>) {
    object.insert(key.into(), value.into());
}
pub fn first(object: &Object, keys: &[&str]) -> String {
    keys.iter()
        .map(|key| text(get(object, key)).trim().to_owned())
        .find(|v| !v.is_empty())
        .unwrap_or_default()
}
pub fn number(value: &Value) -> i64 {
    value
        .as_f64()
        .map(|n| n as i64)
        .unwrap_or_else(|| text(value).trim().parse::<i64>().unwrap_or_default())
}
pub fn duration_text(value: &Value) -> String {
    text(value)
        .trim()
        .to_lowercase()
        .trim_end_matches('s')
        .trim_end_matches('秒')
        .trim()
        .to_owned()
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Channel {
    pub protocol: String,
    pub base_url: String,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Input {
    pub channel: Channel,
    pub model: String,
    pub endpoint: String,
    pub body: Value,
}
fn validate(value: &Value) -> ApiResult<()> {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                if matches!(key.trim().to_lowercase().as_str(), "apikey" | "api_key") {
                    return Err("参数转译请求不能包含 API Key".into());
                }
                validate(value)?;
            }
        }
        Value::Array(values) => {
            for value in values {
                validate(value)?;
            }
        }
        Value::String(s) => {
            let s = s.trim();
            let lower = s.to_lowercase();
            if lower.starts_with("data:") || lower.starts_with("blob:") {
                return Err("参考文件不能传给参数转译接口".into());
            }
            if s.len() > 2048
                && s.chars().all(|c| {
                    c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=' | '\r' | '\n')
                })
            {
                return Err("参数转译请求不能包含 base64 文件内容".into());
            }
        }
        _ => {}
    }
    Ok(())
}
pub fn channel_url(base: &str, path: &str) -> String {
    let mut base = base.trim().trim_end_matches('/').to_owned();
    if let Ok(mut url) = url::Url::parse(&base) {
        let lower = url.path().to_lowercase();
        for version in ["/api/plan/v3", "/api/paas/v4"] {
            if let Some(index) = lower.find(version) {
                let end = index + version.len();
                if lower.len() == end || lower.as_bytes()[end] == b'/' {
                    let path = url.path()[..end].to_owned();
                    url.set_path(&path);
                    url.set_query(None);
                    url.set_fragment(None);
                    base = url.as_str().trim_end_matches('/').to_owned();
                    break;
                }
            }
        }
    }
    let lower = base.to_lowercase();
    if !["/v1", "/api/v3", "/api/plan/v3", "/api/paas/v4"]
        .iter()
        .any(|suffix| lower.ends_with(suffix))
    {
        base.push_str("/v1");
    }
    format!("{base}{path}")
}
fn reference_kinds(value: &Value, kinds: &mut BTreeSet<String>) {
    match value {
        Value::Object(values) => {
            for value in values.values() {
                reference_kinds(value, kinds)
            }
        }
        Value::Array(values) => {
            for value in values {
                reference_kinds(value, kinds)
            }
        }
        Value::String(value) => {
            if let Ok(url) = url::Url::parse(value.trim()) {
                if url.scheme() == "https" && url.host_str() == Some("direct-reference.invalid") {
                    let parts = url.path().trim_matches('/').split('/').collect::<Vec<_>>();
                    if parts.len() == 3
                        && !parts[0].is_empty()
                        && !parts[2].is_empty()
                        && matches!(parts[1], "image" | "video" | "audio")
                    {
                        kinds.insert(parts[1].into());
                    }
                }
            }
        }
        _ => {}
    }
}
pub fn prepare(input: Input) -> ApiResult<Value> {
    let model = input.model.trim();
    let endpoint = input.endpoint.trim();
    let protocol = input.channel.protocol.trim().to_lowercase();
    let base = input.channel.base_url.trim();
    if model.is_empty() {
        return Err("缺少模型名称".into());
    }
    if !matches!(
        endpoint,
        "/images/generations" | "/images/edits" | "/videos"
    ) {
        return Err("当前接口不支持本地参数转译".into());
    }
    let url = url::Url::parse(base).map_err(|_| "渠道地址格式错误")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("渠道地址格式错误".into());
    }
    validate(&input.body)?;
    let is_kie = protocol == "kie"
        || base.to_lowercase().contains("kie.ai")
        || model.to_lowercase().contains("kie/");
    let (provider, body, path) = if is_kie {
        (
            "kie",
            crate::kie::normalize(model, &input.body)?,
            if endpoint == "/images/generations"
                && model.eq_ignore_ascii_case("grok-imagine-image-2-0/text-to-image")
            {
                "/client/tasks"
            } else {
                "/jobs/createTask"
            },
        )
    } else if protocol == "apimart" {
        (
            "apimart",
            crate::apimart::normalize(model, &input.body, endpoint == "/videos")?,
            if endpoint == "/videos" {
                "/videos/generations"
            } else if endpoint == "/images/edits"
                && crate::apimart::model_name(model).contains("grok-imagine")
                && crate::apimart::model_name(model).contains("edit")
            {
                "/images/edits"
            } else {
                "/images/generations"
            },
        )
    } else {
        return Err("当前渠道不支持本地复用后端转译".into());
    };
    let mut kinds = BTreeSet::new();
    reference_kinds(&body, &mut kinds);
    let mut uploads = Object::new();
    for kind in kinds {
        if provider == "kie" {
            uploads.insert(kind.clone(),json!({"url":"https://kieai.redpandaai.co/api/file-stream-upload","fileField":"file","fileNameField":"fileName","extraFields":{"uploadPath":format!("{}s/user-uploads",if kind=="audio"{"audio"}else{&kind})},"responsePaths":["data.downloadUrl","data.fileUrl","data.url"]}));
        } else if kind == "image" {
            uploads.insert(kind,json!({"url":channel_url(base,"/uploads/images"),"fileField":"file","responsePaths":["url"]}));
        } else {
            return Err("APIMart 本地视频和音频参考暂不支持直传，请使用公网媒体地址".into());
        }
    }
    let mut plan = json!({"provider":provider,"url":channel_url(base,path),"contentType":"application/json","body":body});
    if !uploads.is_empty() {
        plan["uploads"] = json!(uploads);
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn direct_plan_rejects_secrets_and_file_payloads() {
        for body in [
            json!({"input":{"API_KEY":"secret"}}),
            json!({"image":"data:image/png;base64,AA=="}),
            json!({"image":"blob:local"}),
        ] {
            assert!(prepare(Input {
                channel: Channel {
                    protocol: "kie".into(),
                    base_url: "https://api.kie.ai".into()
                },
                model: "gpt-image-2-text-to-image".into(),
                endpoint: "/images/generations".into(),
                body
            })
            .is_err());
        }
    }
    #[test]
    fn model_parameters_match_original_plans() {
        let fixtures: Vec<Value> =
            serde_json::from_str(include_str!("../tests/fixtures/direct-plans.json")).unwrap();
        let corrections: Vec<Value> =
            serde_json::from_str(include_str!("../tests/fixtures/direct-plan-fixes.json")).unwrap();
        let mut failures = Vec::new();
        let mut checked = 0;
        for fixture in fixtures {
            checked += 1;
            let input: Input = serde_json::from_value(fixture["input"].clone()).unwrap();
            let actual = prepare(input);
            let expected = corrections
                .iter()
                .find(|c| c["input"] == fixture["input"])
                .unwrap_or(&fixture);
            let matched = match &actual {
                Ok(plan) => expected.get("plan") == Some(plan),
                Err(error) => expected["error"].as_str() == Some(error.as_str()),
            };
            if !matched {
                failures.push(json!({"input":fixture["input"],"expected":fixture.get("plan").unwrap_or(&fixture["error"]),"actual":match actual{Ok(v)=>v,Err(e)=>json!(e)}}));
            }
        }
        if !failures.is_empty() {
            std::fs::write(
                "/tmp/canvas-direct-parity-failures.json",
                serde_json::to_vec_pretty(&failures).unwrap(),
            )
            .unwrap();
        }
        assert!(
            failures.is_empty(),
            "{} / {checked} fixtures differ; details in /tmp/canvas-direct-parity-failures.json",
            failures.len()
        );
    }
}
