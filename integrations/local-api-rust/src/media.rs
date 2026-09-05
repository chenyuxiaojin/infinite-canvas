use crate::{db::ApiResult, prompts::text};
use axum::{
    body::Body,
    http::{HeaderMap, Request, StatusCode},
    response::{IntoResponse, Response},
};
use rusqlite::{Connection, OptionalExtension};
use serde_json::{json, Value};
use std::{io, net::IpAddr, path::PathBuf, time::Duration};
use tower::ServiceExt;
use tower_http::services::ServeFile;

pub fn file_info(db: &Connection, id: &str) -> ApiResult<Value> {
    db.query_row(
        "SELECT * FROM storage_objects WHERE id=?1 AND (deleted_at='' OR deleted_at IS NULL)",
        [id],
        |row| {
            let mut out = json!({});
            for (field, column) in [
                ("id", "id"),
                ("providerId", "provider_id"),
                ("bucket", "bucket"),
                ("objectKey", "object_key"),
                ("publicUrl", "public_url"),
                ("mimeType", "mime_type"),
                ("sha256", "sha256"),
                ("createdBy", "created_by"),
                ("createdAt", "created_at"),
                ("deletedAt", "deleted_at"),
            ] {
                out[field] = json!(text(row, column)?);
            }
            for field in ["bytes", "width", "height"] {
                out[field] = json!(row.get::<_, Option<i64>>(field)?.unwrap_or_default());
            }
            out["direct"] = json!(row.get::<_, Option<bool>>("direct")?.unwrap_or(false));
            Ok(out)
        },
    )
    .optional()
    .map_err(|e| e.to_string())?
    .ok_or_else(|| "文件记录不存在".into())
}
fn blocked(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let a = ip.octets();
            ip.is_loopback()
                || ip.is_private()
                || ip.is_link_local()
                || ip.is_multicast()
                || ip.is_unspecified()
                || a[0] == 0
                || (a[0] == 100 && (64..=127).contains(&a[1]))
        }
        IpAddr::V6(ip) => {
            if let Some(v4) = ip.to_ipv4_mapped() {
                return blocked(v4.into());
            }
            let a = ip.segments();
            ip.is_loopback()
                || ip.is_multicast()
                || ip.is_unspecified()
                || (a[0] & 0xfe00) == 0xfc00
                || (a[0] & 0xffc0) == 0xfe80
        }
    }
}
async fn safe_resolve(host: &str, port: u16) -> ApiResult<Vec<std::net::SocketAddr>> {
    let addresses: Vec<_> = tokio::time::timeout(
        Duration::from_secs(30),
        tokio::net::lookup_host((host, port)),
    )
    .await
    .map_err(|_| "CONNECT_TIMEOUT: 媒体地址解析超时")?
    .map_err(|_| "CONNECT_FAILED: 媒体地址解析失败")?
    .collect();
    if addresses.is_empty() || addresses.iter().any(|a| blocked(a.ip())) {
        return Err("禁止代理本地或内网地址".into());
    }
    Ok(addresses)
}
fn request_error(error: reqwest::Error) -> String {
    if error.is_connect() {
        "CONNECT_FAILED: 媒体连接失败"
    } else if error.is_timeout() {
        "READ_TIMEOUT: 媒体长时间没有返回数据"
    } else {
        "UPSTREAM_DISCONNECTED: 媒体连接中断或服务退出"
    }
    .into()
}
pub async fn upstream(address: &str, headers: &HeaderMap) -> ApiResult<reqwest::Response> {
    let mut url = url::Url::parse(address).map_err(|_| "媒体地址无效")?;
    for redirect in 0..=5 {
        if !matches!(url.scheme(), "http" | "https")
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err("媒体地址无效".into());
        }
        let host = url.host_str().ok_or("媒体地址无效")?;
        let host = host.trim_start_matches('[').trim_end_matches(']');
        let addresses =
            safe_resolve(host, url.port_or_known_default().ok_or("媒体端口无效")?).await?;
        // Resolve each redirect and pin the checked addresses. Never allow system proxy bypass.
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .resolve_to_addrs(host, &addresses)
            .connect_timeout(Duration::from_secs(30))
            .read_timeout(Duration::from_secs(300))
            .pool_max_idle_per_host(0)
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .no_zstd()
            .build()
            .map_err(|_| "媒体客户端初始化失败")?;
        let mut request = client
            .get(url.clone())
            .header("User-Agent", "Mozilla/5.0")
            .header("Accept", "*/*")
            .header("Accept-Encoding", "identity");
        if let Some(range) = headers.get("range") {
            request = request.header("Range", range);
        }
        let response = request.send().await.map_err(request_error)?;
        if response.status().is_redirection() {
            if redirect == 5 {
                return Err("媒体重定向次数过多".into());
            }
            let location = response
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .ok_or("媒体重定向地址无效")?;
            url = url.join(location).map_err(|_| "媒体重定向地址无效")?;
        } else if response.status().is_success()
            || response.status() == StatusCode::RANGE_NOT_SATISFIABLE
        {
            return Ok(response);
        } else {
            return Err(format!("媒体服务返回 {}", response.status().as_u16()));
        }
    }
    unreachable!()
}
pub fn stream(response: reqwest::Response) -> Response {
    use futures_util::StreamExt;
    let mut builder = Response::builder()
        .status(response.status())
        .header("Cache-Control", "public, max-age=86400");
    for name in [
        "content-type",
        "content-length",
        "content-range",
        "accept-ranges",
        "etag",
        "last-modified",
        "content-encoding",
    ] {
        if let Some(value) = response.headers().get(name) {
            builder = builder.header(name, value);
        }
    }
    // No detached blocking reader or buffered producer. Dropping the body drops upstream I/O.
    let body = response
        .bytes_stream()
        .map(|chunk| chunk.map_err(|error| io::Error::other(request_error(error))));
    builder
        .body(Body::from_stream(body))
        .unwrap_or_else(|_| StatusCode::BAD_GATEWAY.into_response())
}
pub async fn reference(directory: PathBuf, id: &str, request: Request<Body>) -> Response {
    if id.is_empty() || id.contains("..") || id.contains('/') || id.contains('\\') {
        return StatusCode::NOT_FOUND.into_response();
    }
    let root = directory.join("reference-media");
    let path = root.join(id);
    if !std::fs::symlink_metadata(&root).is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink())
        || !std::fs::symlink_metadata(&path)
            .is_ok_and(|m| m.is_file() && !m.file_type().is_symlink())
    {
        return StatusCode::NOT_FOUND.into_response();
    }
    match ServeFile::new(path).oneshot(request).await {
        Ok(response) => {
            let mut response = response.into_response();
            response
                .headers_mut()
                .insert("cache-control", "public, max-age=86400".parse().unwrap());
            response
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn proxy_does_not_resolve_local_or_metadata_addresses() {
        for ip in [
            "127.0.0.1",
            "10.2.3.4",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "::1",
            "::ffff:127.0.0.1",
            "fd00::1",
        ] {
            assert!(blocked(ip.parse().unwrap()), "{ip}");
        }
        assert!(!blocked("1.1.1.1".parse().unwrap()));
        assert!(safe_resolve("127.0.0.1", 3101).await.is_err());
    }
    #[tokio::test]
    async fn reference_media_supports_ranges_head_and_rejects_traversal() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("reference-media")).unwrap();
        std::fs::write(dir.path().join("reference-media/clip.mp4"), b"0123456789").unwrap();
        let response = reference(
            dir.path().into(),
            "clip.mp4",
            Request::builder()
                .uri("/api/media/references/clip.mp4")
                .header("Range", "bytes=2-5")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(response.headers()["content-range"], "bytes 2-5/10");
        assert_eq!(
            axum::body::to_bytes(response.into_body(), 100)
                .await
                .unwrap(),
            "2345"
        );
        let head = reference(
            dir.path().into(),
            "clip.mp4",
            Request::builder()
                .method("HEAD")
                .uri("/api/media/references/clip.mp4")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(head.headers()["content-length"], "10");
        assert!(axum::body::to_bytes(head.into_body(), 100)
            .await
            .unwrap()
            .is_empty());
        assert_eq!(
            reference(
                dir.path().into(),
                "../clip.mp4",
                Request::new(Body::empty())
            )
            .await
            .status(),
            StatusCode::NOT_FOUND
        );
    }
}

#[cfg(test)]
mod cancellation_tests {
    use super::*;
    use futures_util::StreamExt;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    struct Released(Arc<AtomicBool>);
    impl Drop for Released {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    #[tokio::test]
    async fn stream_drop_closes_upstream_and_complete_stream_preserves_bytes() {
        let released = Arc::new(AtomicBool::new(false));
        let guard = released.clone();
        let router = axum::Router::new()
            .route(
                "/slow",
                axum::routing::get(move || {
                    let guard = Released(guard.clone());
                    async move {
                        let source = futures_util::stream::unfold(guard, |guard| async move {
                            tokio::time::sleep(Duration::from_millis(10)).await;
                            Some((
                                Ok::<_, io::Error>(axum::body::Bytes::from_static(
                                    b"original-bytes",
                                )),
                                guard,
                            ))
                        });
                        Body::from_stream(source)
                    }
                }),
            )
            .route(
                "/complete",
                axum::routing::get(|| async { "original complete media" }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let client = reqwest::Client::builder()
            .no_proxy()
            .pool_max_idle_per_host(0)
            .build()
            .unwrap();
        let response = client
            .get(format!("http://{address}/slow"))
            .send()
            .await
            .unwrap();
        let mut body = stream(response).into_body().into_data_stream();
        assert_eq!(body.next().await.unwrap().unwrap(), "original-bytes");
        drop(body);
        tokio::time::timeout(Duration::from_secs(2), async {
            while !released.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let response = client
            .get(format!("http://{address}/complete"))
            .send()
            .await
            .unwrap();
        assert_eq!(
            axum::body::to_bytes(stream(response).into_body(), 100)
                .await
                .unwrap(),
            "original complete media"
        );
        server.abort();
    }
    #[tokio::test]
    async fn idle_read_timeout_remains_distinct() {
        let router = axum::Router::new().route(
            "/",
            axum::routing::get(|| async {
                Body::from_stream(futures_util::stream::once(async {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    Ok::<_, io::Error>(axum::body::Bytes::from_static(b"late"))
                }))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let response = reqwest::Client::builder()
            .no_proxy()
            .read_timeout(Duration::from_millis(30))
            .build()
            .unwrap()
            .get(format!("http://{address}"))
            .send()
            .await
            .unwrap();
        let mut body = stream(response).into_body().into_data_stream();
        let error = body.next().await.unwrap().unwrap_err();
        assert!(error.to_string().contains("READ_TIMEOUT"), "{error}");
        server.abort();
    }
}
