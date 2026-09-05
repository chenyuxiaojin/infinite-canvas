use crate::{
    assets,
    db::{self, ApiResult},
    media, prompts,
    query::{json_result, Query},
    settings,
};
use axum::{
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, Path, RawQuery, Request, State},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use serde_json::{json, Value};
use std::{path::PathBuf, sync::Arc};

#[derive(Clone)]
pub struct AppState {
    pub database: PathBuf,
    pub sync_lock: Arc<tokio::sync::Mutex<()>>,
}
impl AppState {
    pub fn new(database: PathBuf) -> Self {
        Self {
            database,
            sync_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }
}
async fn blocking(
    state: AppState,
    action: impl FnOnce(&std::path::Path) -> ApiResult<Value> + Send + 'static,
) -> Response {
    let result = tokio::task::spawn_blocking(move || action(&state.database))
        .await
        .unwrap_or_else(|_| Err("本机服务任务中断，请重试".into()));
    let mut response = Json(json_result(result)).into_response();
    response
        .headers_mut()
        .insert("cache-control", "no-store".parse().unwrap());
    response
}
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(|| async { "ok" }))
        .route(
            "/api/ai/direct-request",
            post(direct_request).layer(DefaultBodyLimit::max(1024 * 1024)),
        )
        .route("/api/settings", get(public_settings))
        .route("/api/storage/config", get(storage_config))
        .route("/api/assets", get(list_assets))
        .route("/api/prompts", get(list_prompts))
        .route("/api/prompts/sync", post(sync_prompts))
        .route("/api/prompts/:id", get(prompt_detail))
        .route(
            "/api/prompt-categories",
            get(categories).post(save_category),
        )
        .route("/api/prompt-categories/:id", delete(delete_category))
        .route("/api/prompt-favorites", post(favorite))
        .route("/api/prompt-favorites/:id", delete(unfavorite))
        .route("/api/files/:id", get(file_info))
        .route("/api/files/:id/content", get(file_content))
        .route("/api/proxy-image", get(proxy_media))
        .route("/api/media/references/:id", get(reference_media))
        .fallback(|| async {
            (
                StatusCode::NOT_FOUND,
                Json(json_result(Err("此接口不属于当前本机版功能".into()))),
            )
        })
        .layer(DefaultBodyLimit::max(2 * 1024 * 1024))
        .layer(middleware::from_fn(local_origin))
        .with_state(state)
}
async fn direct_request(body: Bytes) -> Response {
    let result = if body.is_empty() {
        Err("请求参数不能为空".into())
    } else {
        serde_json::from_slice::<crate::direct::Input>(&body)
            .map_err(|_| "请求参数格式错误".to_owned())
            .and_then(crate::direct::prepare)
    };
    let mut response = Json(json_result(result)).into_response();
    response
        .headers_mut()
        .insert("cache-control", "no-store".parse().unwrap());
    response
}
async fn local_origin(request: Request, next: Next) -> Response {
    if let Some(origin) = request.headers().get("origin") {
        let allowed = [
            "http://127.0.0.1:3100",
            "http://localhost:3100",
            "tauri://localhost",
            "https://tauri.localhost",
        ];
        if !origin
            .to_str()
            .is_ok_and(|origin| allowed.contains(&origin))
        {
            return (
                StatusCode::FORBIDDEN,
                Json(json_result(Err("此来源不能访问本机画布服务".into()))),
            )
                .into_response();
        }
    }
    if let Some(host) = request.headers().get("host").and_then(|v| v.to_str().ok()) {
        if !host.starts_with("127.0.0.1:")
            && !host.starts_with("localhost:")
            && !host.starts_with("[::1]:")
        {
            return StatusCode::FORBIDDEN.into_response();
        }
    }
    next.run(request).await
}
async fn public_settings(State(state): State<AppState>) -> Response {
    blocking(state, |path| settings::public(&db::connect(path)?)).await
}
async fn storage_config(State(state): State<AppState>) -> Response {
    blocking(state, |path| settings::storage(&db::connect(path)?)).await
}
async fn list_assets(State(state): State<AppState>, RawQuery(raw): RawQuery) -> Response {
    blocking(state, move |path| {
        assets::list(&db::connect(path)?, &Query::parse(raw.as_deref()))
    })
    .await
}
async fn list_prompts(State(state): State<AppState>, RawQuery(raw): RawQuery) -> Response {
    blocking(state, move |path| {
        prompts::list(&db::connect(path)?, &Query::parse(raw.as_deref()))
    })
    .await
}
async fn categories(State(state): State<AppState>) -> Response {
    blocking(state, |path| {
        Ok(json!(prompts::categories(&db::connect(path)?)?))
    })
    .await
}
async fn save_category(State(state): State<AppState>, body: Bytes) -> Response {
    blocking(state, move |path| {
        let item = serde_json::from_slice(&body).map_err(|_| "订阅源格式无效")?;
        Ok(json!(prompts::save_category(&db::connect(path)?, item)?))
    })
    .await
}
async fn delete_category(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    blocking(state, move |path| {
        let mut db = db::connect(path)?;
        let tx = db.transaction().map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM prompt_categories WHERE category=?1", [&id])
            .map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM prompts WHERE category=?1", [&id])
            .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(json!(true))
    })
    .await
}
async fn prompt_detail(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    blocking(state, move |path| Ok(json!(prompts::detail(path, &id)?))).await
}
async fn favorite(State(state): State<AppState>, body: Bytes) -> Response {
    blocking(state, move |path| {
        prompts::favorite(
            &db::connect(path)?,
            serde_json::from_slice(&body).map_err(|_| "收藏内容无效或过大")?,
        )
    })
    .await
}
async fn unfavorite(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    blocking(state, move |path| {
        db::connect(path)?
            .execute("DELETE FROM prompt_favorites WHERE id=?1", [id])
            .map_err(|e| e.to_string())?;
        Ok(json!(true))
    })
    .await
}
async fn sync_prompts(State(state): State<AppState>, RawQuery(raw): RawQuery) -> Response {
    let guard = state.sync_lock.clone().lock_owned().await;
    blocking(state, move |path| {
        let _guard = guard;
        let category = Query::parse(raw.as_deref()).category;
        if category.is_empty() {
            prompts::sync_all(path)
        } else {
            Ok(json!(prompts::sync(path, &category)?))
        }
    })
    .await
}
async fn file_info(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    blocking(state, move |path| {
        media::file_info(&db::connect(path)?, &id)
    })
    .await
}
async fn file_content(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let result = tokio::task::spawn_blocking(move || {
        let object = media::file_info(&db::connect(&state.database)?, &id)?;
        let url = object["publicUrl"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or("此历史素材没有可读取的公开地址，请从原存储导入本机")?;
        Ok(url.to_owned())
    })
    .await
    .unwrap_or_else(|_| Err("读取媒体失败".into()));
    let result = match result { Ok(url) => media::upstream(&url, &headers).await, Err(error) => Err(error) };
    match result {
        Ok(response) => media::stream(response),
        Err(msg) => media_failure(msg),
    }
}
async fn proxy_media(RawQuery(raw): RawQuery, headers: HeaderMap) -> Response {
    let url = url::form_urlencoded::parse(raw.unwrap_or_default().as_bytes())
        .find(|(k, _)| k == "url")
        .map(|(_, v)| v.into_owned())
        .unwrap_or_default();
    let result = media::upstream(&url, &headers).await;
    match result {
        Ok(response) => media::stream(response),
        Err(msg) => media_failure(msg),
    }
}
async fn reference_media(
    State(state): State<AppState>,
    Path(id): Path<String>,
    request: Request<Body>,
) -> Response {
    media::reference(state.database.parent().unwrap().to_path_buf(), &id, request).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower::ServiceExt;
    #[tokio::test]
    async fn direct_request_http_is_metadata_only_and_bounded() {
        let app = router(AppState::new(PathBuf::from("/unopened-database")));
        let input = json!({"channel":{"protocol":"apimart","baseUrl":"https://api.apimart.ai"},"model":"gpt-image-2","endpoint":"/images/edits","body":{"prompt":"test","image_urls":["https://direct-reference.invalid/test/image/0"]}});
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/ai/direct-request")
                    .body(Body::from(input.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let result: Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), 1024 * 1024)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(result["code"], 0);
        assert_eq!(
            result["data"]["url"],
            "https://api.apimart.ai/v1/images/generations"
        );
        assert_eq!(
            result["data"]["uploads"]["image"]["responsePaths"],
            json!(["url"])
        );
        for body in ["{".to_owned(),String::new(),json!({"channel":{"protocol":"kie","baseUrl":"https://api.kie.ai"},"model":"test","endpoint":"/videos","body":{"image":"blob:local"}}).to_string()] {
            let response=app.clone().oneshot(Request::builder().method("POST").uri("/api/ai/direct-request").body(Body::from(body)).unwrap()).await.unwrap();
            let result:Value=serde_json::from_slice(&axum::body::to_bytes(response.into_body(),1024*1024).await.unwrap()).unwrap();assert_eq!(result["code"],1);
        }
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/ai/direct-request")
                    .body(Body::from(vec![b' '; 1024 * 1024 + 1]))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }
    #[tokio::test]
    async fn http_keeps_envelope_and_rejects_cross_origin_mutations() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("api.db");
        db::initialize(&path).unwrap();
        let app = router(AppState::new(path));
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/prompts")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), 1024 * 1024)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(json["code"], 0);
        assert_eq!(json["data"]["total"], 0);
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/prompt-categories")
                    .header("Origin", "https://unrelated.example")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
}

fn media_failure(msg: String) -> Response {
    let kind=if msg.starts_with("READ_TIMEOUT") {"read_timeout"} else if msg.starts_with("UPSTREAM_DISCONNECTED") {"service_exited"} else {"connect_failed"};
    let status=if kind=="read_timeout" {StatusCode::GATEWAY_TIMEOUT} else {StatusCode::BAD_GATEWAY};
    (status,Json(json!({"code":1,"data":null,"msg":msg,"kind":kind,"submitted":false}))).into_response()
}
