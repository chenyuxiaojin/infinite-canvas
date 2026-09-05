//! Existing subscription formats, parsed transiently. Only catalog metadata is persisted.
use crate::{
    db::{self, ApiResult},
    prompts::{Category, Prompt},
};
use regex::Regex;
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::Read,
};

const DOCUMENT_LIMIT: u64 = 20 * 1024 * 1024;
fn re(pattern: &str) -> Regex {
    Regex::new(pattern).expect("static prompt source expression")
}
fn first(value: &str, pattern: &str) -> String {
    re(pattern)
        .captures(value)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_owned())
        .unwrap_or_default()
}
fn string<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or_default()
}
fn nonempty<'a>(values: impl IntoIterator<Item = &'a str>) -> String {
    values
        .into_iter()
        .map(str::trim)
        .find(|s| !s.is_empty())
        .unwrap_or_default()
        .to_owned()
}
fn read_document(reader: impl Read) -> ApiResult<String> {
    let mut bytes = Vec::new();
    reader
        .take(DOCUMENT_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "读取提示词来源失败")?;
    if bytes.len() as u64 > DOCUMENT_LIMIT {
        return Err("来源文档超过 20 MB，请改用单个提示词文档链接".into());
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}
fn fetch(url: &str) -> ApiResult<String> {
    let url = url::Url::parse(url).map_err(|_| "提示词来源地址无效")?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("提示词来源地址无效".into());
    }
    let response = ureq::get(url.as_str())
        .timeout(std::time::Duration::from_secs(30))
        .call()
        .map_err(|_| "拉取提示词来源失败")?;
    read_document(response.into_reader())
}
fn parts(markdown: &str, prefix: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = Vec::new();
    for line in markdown.split('\n') {
        if line.starts_with(prefix) && !current.is_empty() {
            result.push(current.join("\n"));
            current.clear();
        }
        current.push(line);
    }
    result.push(current.join("\n"));
    result
}
fn absolute(base: &str, image: &str) -> String {
    if image.is_empty() || image.starts_with("http://") || image.starts_with("https://") {
        image.into()
    } else {
        format!(
            "{base}/{}",
            image
                .strip_prefix('.')
                .unwrap_or(image)
                .trim_start_matches('/')
        )
    }
}
fn images(base: &str, block: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for pattern in [r#"<img[^>]+src="([^"]+)""#, r"!\[[^\]]*\]\(([^)]+)\)"] {
        for matched in re(pattern).captures_iter(block) {
            let image = absolute(base, &matched[1]);
            if !image.is_empty() && seen.insert(image.clone()) {
                out.push(image);
            }
        }
    }
    out
}
fn tags(value: &str, pattern: &str) -> Vec<String> {
    re(pattern)
        .split(value)
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}
fn heading_tags(value: &str) -> Vec<String> {
    tags(
        &re(r"[^\p{L}\p{N}/&、与 ]").replace_all(value, ""),
        r"\s*(/|&|、|与)\s*",
    )
}
fn preview(images: &[String]) -> String {
    images
        .iter()
        .map(|image| format!("![]({image})"))
        .collect::<Vec<_>>()
        .join("\n\n")
}
fn item(title: String, prompt: String, tags: Vec<String>, images: Vec<String>) -> Prompt {
    Prompt {
        title,
        prompt: prompt.trim().into(),
        tags,
        cover_url: images.first().cloned().unwrap_or_default(),
        preview: preview(&images),
        ..Prompt::default()
    }
}
fn normalized_date(value: &str) -> String {
    use time::format_description::well_known::{Rfc2822, Rfc3339};
    let value = value.trim();
    if value.is_empty() {
        return String::new();
    }
    if let Ok(parsed) = time::OffsetDateTime::parse(value, &Rfc3339)
        .or_else(|_| time::OffsetDateTime::parse(value, &Rfc2822))
    {
        return parsed
            .replace_nanosecond(0)
            .unwrap()
            .format(&Rfc3339)
            .unwrap();
    }
    if let Ok(format) = time::format_description::parse("[year]-[month]-[day]") {
        if let Ok(date) = time::Date::parse(value, &format) {
            return date.midnight().assume_utc().format(&Rfc3339).unwrap();
        }
    }
    value.into()
}

pub fn load(source: &Category) -> ApiResult<Vec<Prompt>> {
    if source.source_type == "local_markdown" || source.path_or_url.starts_with('/') {
        let file = File::open(&source.path_or_url).map_err(|_| "读取本地提示词文件失败")?;
        return Ok(markdown(
            &read_document(file)?,
            &source.category,
            &source.path_or_url,
        ));
    }
    if source.source_type == "custom_url"
        || source.path_or_url.starts_with("http://")
        || source.path_or_url.starts_with("https://")
    {
        return Ok(markdown(
            &fetch(&source.path_or_url)?,
            &source.category,
            &source.path_or_url,
        ));
    }
    load_builtin(&source.category, |base, file| {
        fetch(&format!("{base}/{file}"))
    })
}

pub fn markdown(text: &str, category: &str, source: &str) -> Vec<Prompt> {
    let mut out = Vec::new();
    let mut title = String::new();
    let mut lines = Vec::new();
    let mut code = Vec::new();
    let mut in_code = false;
    let flush =
        |out: &mut Vec<Prompt>, title: &mut String, lines: &mut Vec<&str>, code: &mut Vec<&str>| {
            if !title.is_empty() {
                let prompt = if code.is_empty() {
                    lines.join("\n")
                } else {
                    code.join("\n")
                }
                .trim()
                .to_owned();
                if prompt.len() > 10 {
                    let clean = re(r"\[(.*?)\]\(.*?\)")
                        .replace_all(title, "$1")
                        .trim_start_matches(|c| "#*- 0123456789.:：".contains(c))
                        .trim()
                        .to_owned();
                    let title_clean = if clean.is_empty() {
                        format!("提示词 {}", out.len() + 1)
                    } else {
                        clean
                    };
                    let content = format!("{prompt} {title}").to_lowercase();
                    let mut tags = vec!["视频创作".to_owned()];
                    for (words, tag) in [
                        (&["shot", "lens", "镜头", "机位"][..], "运镜机位"),
                        (&["close-up", "特写", "wide", "全景"][..], "景别设计"),
                        (&["lighting", "光影", "neon"][..], "光影视效"),
                        (&["seedance"][..], "Seedance"),
                        (&["minimax", "h3"][..], "MiniMax-H3"),
                    ] {
                        if words.iter().any(|w| content.contains(w)) {
                            tags.push(tag.into());
                        }
                    }
                    let mut short =
                        String::from_utf8_lossy(&prompt.as_bytes()[..prompt.len().min(120)])
                            .into_owned();
                    if prompt.len() > 120 {
                        short.push_str("...");
                    }
                    out.push(Prompt {
                        id: format!("{category}-{}", out.len() + 1),
                        title: title_clean,
                        prompt,
                        tags,
                        category: category.into(),
                        github_url: source.into(),
                        preview: short,
                        created_at: db::now(),
                        updated_at: db::now(),
                        ..Prompt::default()
                    });
                }
            }
            title.clear();
            lines.clear();
            code.clear();
        };
    for line in text.split('\n') {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            code.push(line);
            continue;
        }
        if trimmed.starts_with("## ") || trimmed.starts_with("### ") {
            flush(&mut out, &mut title, &mut lines, &mut code);
            title = trimmed
                .strip_prefix("### ")
                .or_else(|| trimmed.strip_prefix("## "))
                .unwrap()
                .into();
            continue;
        }
        if !title.is_empty() && !trimmed.is_empty() && !trimmed.starts_with("---") {
            lines.push(line);
        }
    }
    flush(&mut out, &mut title, &mut lines, &mut code);
    out
}

// The fetch function is injected in tests, so each original format can be compared offline.
pub fn load_builtin(
    category: &str,
    fetch: impl Fn(&str, &str) -> ApiResult<String>,
) -> ApiResult<Vec<Prompt>> {
    let base = match category {
        "gpt-image-2-prompts" => {
            "https://raw.githubusercontent.com/tigerowo/awesome-gpt-image-2-prompts/main"
        }
        "awesome-gpt-image" => "https://raw.githubusercontent.com/ZeroLu/awesome-gpt-image/main",
        "awesome-gpt4o-image-prompts" => {
            "https://raw.githubusercontent.com/ImgEdify/Awesome-GPT4o-Image-Prompts/main"
        }
        "xianyu-awesome-gptimage2" => {
            "https://raw.githubusercontent.com/xianyu110/awesome-gptimage2/main"
        }
        "youmind-gpt-image-2" => {
            "https://raw.githubusercontent.com/YouMind-OpenLab/awesome-gpt-image-2/main"
        }
        "youmind-nano-banana-pro" => {
            "https://raw.githubusercontent.com/YouMind-OpenLab/awesome-nano-banana-pro-prompts/main"
        }
        "davidwu-gpt-image2-prompts" => {
            "https://raw.githubusercontent.com/davidwuw0811-boop/awesome-gpt-image2-prompts/main"
        }
        _ => return Err("未知提示词分类".into()),
    };
    let mut out = Vec::new();
    match category {
        "gpt-image-2-prompts" => {
            let data: Value = serde_json::from_str(&fetch(base, "data/ingested_tweets.json")?)
                .map_err(|_| "来源数据格式无效")?;
            let mut cases = HashMap::new();
            let case_re = re(
                r"(?s)### Case \d+: \[[^\]]+\]\(([^)]+)\).*?\*\*Prompt:\*\*\s*\r?\n\s*```[\w-]*\r?\n(.*?)\r?\n```",
            );
            let image_re = re(r#"<img[^>]+src="([^"]+)"|!\[[^\]]*\]\(([^)]+)\)"#);
            for file in [
                "README.md",
                "cases/ad-creative.md",
                "cases/character.md",
                "cases/comparison.md",
                "cases/ecommerce.md",
                "cases/portrait.md",
                "cases/poster.md",
                "cases/ui.md",
            ] {
                for captured in case_re.captures_iter(&fetch(base, file)?) {
                    let image = image_re
                        .captures(&captured[0])
                        .and_then(|m| {
                            m.get(1)
                                .or_else(|| m.get(2))
                                .map(|s| absolute(base, s.as_str()))
                        })
                        .unwrap_or_default();
                    let value = (captured[2].trim().to_owned(), image);
                    cases.insert(captured[1].to_owned(), value.clone());
                    if let Some(dir) = re(r"images/\w+_case\d+").find(&captured[0]) {
                        cases.insert(dir.as_str().into(), value);
                    }
                }
            }
            for record in data["records"].as_array().ok_or("来源记录无效")? {
                let Some((prompt, image)) = cases
                    .get(string(record, "tweet_url"))
                    .or_else(|| cases.get(string(record, "image_dir")))
                else {
                    continue;
                };
                if prompt.is_empty() {
                    continue;
                }
                let tags = tags(
                    &re(r"(?i)\s+Cases$").replace_all(string(record, "category"), ""),
                    r"\s*(&|and)\s*",
                );
                let date = normalized_date(string(record, "added_at"));
                let mut p = item(
                    string(record, "title").into(),
                    prompt.clone(),
                    tags,
                    vec![image.clone()],
                );
                p.created_at = date.clone();
                p.updated_at = date;
                out.push(p);
            }
        }
        "awesome-gpt-image" => {
            for section in parts(&fetch(base, "README.md")?, "## ") {
                let tags = heading_tags(&first(&section, r"(?m)^##\s+(.+)$"));
                for block in parts(&section, "### ") {
                    let title = re(r"\[([^\]]+)\]\([^)]+\)")
                        .replace_all(&first(&block, r"(?m)^###\s+(.+)$"), "$1")
                        .trim()
                        .to_owned();
                    let prompt = first(
                        &block,
                        r"(?s)\*\*Prompt:\*\*\s*\r?\n\s*```[\w-]*\r?\n(.*?)\r?\n```",
                    );
                    if !title.is_empty() && !prompt.trim().is_empty() {
                        out.push(item(title, prompt, tags.clone(), images(base, &block)));
                    }
                }
            }
        }
        "awesome-gpt4o-image-prompts" | "youmind-gpt-image-2" | "youmind-nano-banana-pro" => {
            let gpt4o = category == "awesome-gpt4o-image-prompts";
            let document = fetch(
                base,
                if gpt4o {
                    "README.zh-CN.md"
                } else {
                    "README_zh.md"
                },
            )?;
            for block in parts(&document, "### ") {
                let title = first(
                    &block,
                    if gpt4o {
                        r"(?m)^###\s+(.+)$"
                    } else {
                        r"(?m)^###\s+No\.\s*\d+:\s*(.+)$"
                    },
                )
                .trim()
                .to_owned();
                let prompt = first(
                    &block,
                    if gpt4o {
                        r"(?s)- \*\*提示词文本：\*\*\s*`(.*?)`"
                    } else {
                        r"(?s)#### .*?提示词\s*\r?\n\s*```[\w-]*\r?\n(.*?)\r?\n```"
                    },
                );
                if title.is_empty() || prompt.trim().is_empty() {
                    continue;
                }
                let mut tags = vec![if gpt4o {
                    "gpt4o"
                } else if category == "youmind-gpt-image-2" {
                    "gpt-image-2"
                } else {
                    "nano-banana-pro"
                }
                .into()];
                if !gpt4o {
                    if let Some((head, _)) = title.split_once(" - ") {
                        tags.extend(heading_tags(head));
                    }
                }
                out.push(item(title, prompt, tags, images(base, &block)));
            }
        }
        "xianyu-awesome-gptimage2" => {
            out = xianyu_collection(&fetch(base, "README.md")?, base);
            let data: Value = serde_json::from_str(&fetch(base, "data/latest-prompts.json")?)
                .map_err(|_| "来源数据格式无效")?;
            let mut records = Vec::new();
            if let Some(groups) = data["dates"].as_array() {
                for group in groups {
                    if let Some(items) = group["items"].as_array() {
                        records.extend(items);
                    }
                }
            }
            if let Some(items) = data["items"].as_array() {
                records.extend(items);
            }
            let mut seen = HashSet::new();
            for record in records {
                let prompt = string(record, "prompt").trim();
                if prompt.is_empty() {
                    continue;
                }
                let fallback = format!(
                    "{}{}{prompt}",
                    string(record, "author"),
                    string(record, "created_at")
                );
                let key = nonempty([string(record, "x_url"), string(record, "url"), &fallback]);
                if !seen.insert(key) {
                    continue;
                }
                let image_urls: Vec<String> = record["image_urls"]
                    .as_array()
                    .map(|xs| {
                        xs.iter()
                            .filter_map(Value::as_str)
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .map(str::to_owned)
                            .collect()
                    })
                    .unwrap_or_default();
                let image = nonempty([
                    string(record, "primary_image_url"),
                    image_urls.first().map(String::as_str).unwrap_or_default(),
                ]);
                let title = nonempty([
                    string(record, "reason"),
                    string(record, "author"),
                    "X Prompt",
                ]);
                let date = normalized_date(string(record, "created_at"));
                let mut lines = Vec::new();
                let link = nonempty([string(record, "x_url"), string(record, "url")]);
                if !link.is_empty() {
                    lines.push(link);
                }
                lines.extend(image_urls);
                if lines.len() == 1 && !image.is_empty() {
                    lines.push(image.clone());
                }
                out.push(Prompt {
                    title,
                    prompt: prompt.into(),
                    cover_url: image,
                    tags: vec!["x".into()],
                    preview: lines.join("\n"),
                    created_at: date.clone(),
                    updated_at: date,
                    ..Prompt::default()
                });
            }
        }
        "davidwu-gpt-image2-prompts" => {
            let data: Value = serde_json::from_str(&fetch(base, "prompts.json")?)
                .map_err(|_| "来源数据格式无效")?;
            for record in data.as_array().ok_or("来源记录无效")? {
                let title = nonempty([string(record, "title_cn"), string(record, "title_en")]);
                let prompt = string(record, "prompt").trim();
                if title.is_empty() || prompt.is_empty() {
                    continue;
                }
                let image = absolute(base, string(record, "image"));
                let mut tags = tags(
                    &["category_cn", "category", "author", "source"]
                        .map(|k| string(record, k))
                        .join("/"),
                    "/",
                );
                if record["needs_ref"].as_bool() == Some(true) {
                    tags.push("需要参考图".into());
                }
                let mut lines = Vec::new();
                for k in ["title_en", "note"] {
                    let s = string(record, k);
                    if !s.is_empty() {
                        lines.push(s.into());
                    }
                }
                if !image.is_empty() {
                    lines.push(format!("![]({image})"));
                }
                out.push(Prompt {
                    id: format!(
                        "{category}-{:03}",
                        record["id"].as_i64().unwrap_or_default()
                    ),
                    title,
                    prompt: prompt.into(),
                    tags,
                    cover_url: image,
                    preview: lines.join("\n\n"),
                    ..Prompt::default()
                });
            }
        }
        _ => unreachable!(),
    }
    for (index, p) in out.iter_mut().enumerate() {
        if p.id.is_empty() {
            p.id = format!("{category}-{:03}", index + 1);
        }
    }
    Ok(out)
}

fn xianyu_collection(document: &str, base: &str) -> Vec<Prompt> {
    let Some(start) = document.find("## 提示词合集") else {
        return Vec::new();
    };
    let rest = &document[start + "## 提示词合集".len()..];
    let end = rest.find("## 高级技巧").unwrap_or(rest.len());
    let rest = &rest[..end];
    let mut out = Vec::new();
    let mut category = String::new();
    let mut title = String::new();
    let mut lines = Vec::new();
    let flush = |out: &mut Vec<Prompt>, title: &str, category: &str, lines: &[&str]| {
        if title.is_empty() || category == "补充案例提示词" {
            return;
        }
        let block = lines.join("\n");
        let mut code = Vec::new();
        let mut in_code = false;
        for line in lines {
            if line.trim().starts_with("```") {
                if in_code {
                    break;
                }
                in_code = true;
                continue;
            }
            if in_code {
                code.push(*line);
            }
        }
        let mut prompt = code.join("\n").trim().to_owned();
        if prompt.is_empty() {
            let mut text = Vec::new();
            for line in lines {
                let s = line.trim();
                if s.is_empty()
                    || [
                        "#",
                        "---",
                        "![",
                        "|",
                        ">",
                        "```",
                        "- 原文链接",
                        "- 公众号",
                        "- 作者",
                        "- 本次补充",
                        "- 说明",
                    ]
                    .iter()
                    .any(|p| s.starts_with(p))
                {
                    continue;
                }
                let s = s.strip_prefix('-').unwrap_or(s).trim();
                let s = s.strip_prefix('*').unwrap_or(s).trim();
                let s = s.strip_prefix("提示词：").unwrap_or(s).trim();
                if !s.is_empty() && !s.starts_with("http") {
                    text.push(s);
                }
            }
            prompt = text.join("\n");
        }
        if prompt.is_empty() {
            return;
        }
        let mut values = vec!["gpt-image-2".into()];
        values.extend(tags(category, r"\s*(/|&|、|与)\s*"));
        out.push(item(title.into(), prompt, values, images(base, &block)));
    };
    for line in rest.split('\n') {
        if let Some(heading) = line.strip_prefix("### ") {
            flush(&mut out, &title, &category, &lines);
            title.clear();
            lines.clear();
            category = heading.trim().into();
            for separator in ["、", ".", "．", " "] {
                if let Some((prefix, value)) = category.split_once(separator) {
                    if !prefix.trim().is_empty() && prefix.trim().chars().count() <= 4 {
                        category = value.trim().into();
                    }
                    break;
                }
            }
        } else if let Some(heading) = line.strip_prefix("#### ") {
            flush(&mut out, &title, &category, &lines);
            lines.clear();
            title = heading.trim().into();
            if let Some((prefix, value)) = title.split_once(' ') {
                if prefix.contains('.') || prefix.contains('．') {
                    title = value.trim().into();
                }
            }
        } else if !title.is_empty() {
            lines.push(line);
        }
    }
    flush(&mut out, &title, &category, &lines);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn markdown_preserves_multiline_prompt_and_headings_inside_code() {
        let items=markdown("## 1. [镜头](https://example.com)\n介绍\n```text\nwide shot\n### this is prompt text\n```\n## 忽略\n短\n","source","local.md");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "镜头");
        assert_eq!(items[0].prompt, "wide shot\n### this is prompt text");
        assert!(items[0].tags.contains(&"运镜机位".into()));
    }
    #[test]
    fn builtin_formats_preserve_content_identity() {
        let formats=[
            ("awesome-gpt-image","## Style\n### [Title](https://example.com)\n**Prompt:**\n```text\nExact text.\n```"),
            ("awesome-gpt4o-image-prompts","### Title\n- **提示词文本：** `Exact text.`"),
            ("youmind-gpt-image-2","### No. 1: Title\n#### 提示词\n```text\nExact text.\n```"),
            ("youmind-nano-banana-pro","### No. 1: Title\n#### 提示词\n```text\nExact text.\n```"),
            ("davidwu-gpt-image2-prompts",r#"[{"id":1,"title_cn":"Title","prompt":"Exact text."}]"#),
        ];
        for (category, body) in formats {
            let items = load_builtin(category, |_, _| Ok(body.into())).unwrap();
            assert_eq!(items.len(), 1, "{category}");
            assert_eq!(items[0].title, "Title");
            assert_eq!(items[0].prompt, "Exact text.");
        }
        let items=load_builtin("xianyu-awesome-gptimage2",|_,file|Ok(if file.ends_with(".json"){"{\"items\":[]}"}else{"## 提示词合集\n### 一、人物\n#### 1.1 Title\n```text\nExact text.\n```\n## 高级技巧"}.into())).unwrap();
        assert_eq!(items[0].title, "Title");
        assert_eq!(items[0].prompt, "Exact text.");
        let items=load_builtin("gpt-image-2-prompts",|_,file|Ok(if file.ends_with(".json"){r#"{"records":[{"title":"Title","tweet_url":"https://example.com","category":"Art Cases"}]}"#}else{"### Case 1: [Title](https://example.com)\n**Prompt:**\n```text\nExact text.\n```"}.into())).unwrap();
        assert_eq!(items[0].title, "Title");
        assert_eq!(items[0].prompt, "Exact text.");
    }
}
