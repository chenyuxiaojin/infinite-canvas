use serde_json::Value;

#[derive(Default, Debug)]
pub struct Query {
    pub keyword: String,
    pub category: String,
    pub kind: String,
    pub tags: Vec<String>,
    pub page: usize,
    pub page_size: usize,
    pub favorites: bool,
}

impl Query {
    pub fn parse(raw: Option<&str>) -> Self {
        let mut query = Self {
            page: 1,
            page_size: 20,
            ..Self::default()
        };
        for (key, value) in url::form_urlencoded::parse(raw.unwrap_or_default().as_bytes()) {
            match key.as_ref() {
                "keyword" => query.keyword = value.into_owned(),
                "category" => query.category = value.into_owned(),
                "type" => query.kind = value.into_owned(),
                "tag" => query.tags.push(value.into_owned()),
                "page" => query.page = value.parse::<usize>().unwrap_or(1).max(1),
                "pageSize" => {
                    query.page_size = value
                        .parse::<usize>()
                        .ok()
                        .filter(|v| *v > 0)
                        .unwrap_or(20)
                        .min(500)
                }
                "favorites" => query.favorites = value == "true",
                _ => {}
            }
        }
        query
    }
    pub fn matches_tags(&self, tags: &[String]) -> bool {
        self.tags.is_empty() || self.tags.iter().any(|tag| tags.contains(tag))
    }
    pub fn bounds(&self) -> (usize, usize) {
        (
            self.page.saturating_sub(1).saturating_mul(self.page_size),
            self.page_size,
        )
    }
}

pub fn json_result(result: Result<Value, String>) -> Value {
    match result {
        Ok(data) => serde_json::json!({"code":0,"data":data,"msg":"ok"}),
        Err(msg) => serde_json::json!({"code":1,"data":null,"msg":msg}),
    }
}

// Existing multi-tag semantics are OR, with exact, case-sensitive string membership.
// Malformed JSON is treated as empty; prompts additionally require an all-string array.
pub fn tag_filter(strict: bool) -> String {
    let valid = "CASE WHEN json_valid(tags) THEN CASE WHEN json_type(tags)='array' THEN tags ELSE '[]' END ELSE '[]' END";
    let strict_check = if strict {
        format!("AND NOT EXISTS(SELECT 1 FROM json_each({valid}) WHERE type<>'text')")
    } else {
        String::new()
    };
    format!("(json_array_length(?4)=0 OR (EXISTS(SELECT 1 FROM json_each({valid}) t JOIN json_each(?4) q ON t.value=q.value AND t.type='text') {strict_check}))")
}
