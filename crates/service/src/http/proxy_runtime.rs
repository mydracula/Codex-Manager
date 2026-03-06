use axum::body::{to_bytes, Body, Bytes};
use axum::extract::{OriginalUri, Path, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, Request as HttpRequest, Response, StatusCode};
use axum::response::{Html, IntoResponse};
use axum::routing::{any, get, post};
use axum::Router;
use reqwest::Client;
use std::io;
use std::sync::Arc;

use crate::http::proxy_bridge::run_proxy_server;
use crate::http::proxy_request::{build_target_url, filter_request_headers};
use crate::http::proxy_response::{merge_upstream_headers, text_response};

#[derive(Clone)]
struct ProxyState {
    backend_base_url: String,
    client: Client,
    rpc_token: String,
    missing_ui_html: Arc<String>,
}

fn build_backend_base_url(backend_addr: &str) -> String {
    format!("http://{backend_addr}")
}

fn is_json_content_type(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(|value| value.trim().eq_ignore_ascii_case("application/json"))
        .unwrap_or(false)
}

fn is_hop_by_hop_header(name: &axum::http::HeaderName) -> bool {
    matches!(
        name.as_str().to_ascii_lowercase().as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
            | "content-length"
            | "content-encoding"
    )
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn builtin_missing_ui_html(detail: &str) -> String {
    let detail = escape_html(detail);
    format!(
        r#"<!doctype html>
<html lang="zh-CN">
  <head>
    <meta charset="utf-8"/>
    <meta name="viewport" content="width=device-width, initial-scale=1"/>
    <title>CodexManager Web</title>
    <style>
      body {{ font-family: ui-sans-serif, system-ui, -apple-system, Segoe UI, Roboto, Helvetica, Arial; padding: 40px; line-height: 1.5; color: #111; }}
      .box {{ max-width: 860px; margin: 0 auto; border: 1px solid #e5e7eb; border-radius: 12px; padding: 20px 24px; background: #fafafa; }}
      h1 {{ margin: 0 0 8px; font-size: 20px; }}
      p {{ margin: 10px 0; color: #374151; }}
      code {{ background: #111827; color: #f9fafb; padding: 2px 6px; border-radius: 6px; }}
    </style>
  </head>
  <body>
    <div class="box">
      <h1>前端资源未就绪</h1>
      <p>当前 <code>codexmanager-service</code> 没有找到可用的前端静态资源。</p>
      <p>详情：<code>{detail}</code></p>
      <p>解决方式：先执行 <code>pnpm -C apps build</code>，或使用带 <code>embedded-ui</code> 特性的发行构建。</p>
    </div>
  </body>
</html>
"#
    )
}

async fn rpc_proxy(
    State(state): State<ProxyState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response<Body> {
    if !is_json_content_type(&headers) {
        return (StatusCode::UNSUPPORTED_MEDIA_TYPE, "{}").into_response();
    }
    let resp = state
        .client
        .post(format!("{}/rpc", state.backend_base_url))
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-codexmanager-rpc-token", &state.rpc_token)
        .body(body)
        .send()
        .await;
    let resp = match resp {
        Ok(v) => v,
        Err(err) => {
            let msg = format!("upstream error: {err}");
            return (StatusCode::BAD_GATEWAY, msg).into_response();
        }
    };

    let status = resp.status();
    let bytes = match resp.bytes().await {
        Ok(v) => v,
        Err(err) => {
            let msg = format!("upstream read error: {err}");
            return (StatusCode::BAD_GATEWAY, msg).into_response();
        }
    };
    let mut out = Response::new(Body::from(bytes));
    *out.status_mut() = status;
    out.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    out
}

async fn gateway_proxy(
    State(state): State<ProxyState>,
    method: Method,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    body: Bytes,
) -> Response<Body> {
    let path_and_query = uri.path_and_query().map(|value| value.as_str()).unwrap_or(uri.path());
    let upstream_url = format!("{}{}", state.backend_base_url, path_and_query);
    let mut req = state.client.request(method, upstream_url);
    for (name, value) in &headers {
        if name.as_str().eq_ignore_ascii_case("host") || is_hop_by_hop_header(name) {
            continue;
        }
        req = req.header(name, value);
    }
    let resp = req.body(body).send().await;
    let resp = match resp {
        Ok(v) => v,
        Err(err) => {
            let msg = format!("upstream error: {err}");
            return (StatusCode::BAD_GATEWAY, msg).into_response();
        }
    };

    let status = resp.status();
    let resp_headers = resp.headers().clone();
    let bytes = match resp.bytes().await {
        Ok(v) => v,
        Err(err) => {
            let msg = format!("upstream read error: {err}");
            return (StatusCode::BAD_GATEWAY, msg).into_response();
        }
    };
    let mut out = Response::new(Body::from(bytes));
    *out.status_mut() = status;
    for (name, value) in &resp_headers {
        if is_hop_by_hop_header(name) {
            continue;
        }
        out.headers_mut().insert(name, value.clone());
    }
    out
}

async fn serve_missing_ui(State(state): State<ProxyState>) -> Html<String> {
    Html((*state.missing_ui_html).clone())
}

async fn serve_embedded_index() -> Response<Body> {
    serve_embedded_path("index.html")
}

async fn serve_embedded_asset(Path(path): Path<String>) -> Response<Body> {
    serve_embedded_path(&path)
}

fn serve_embedded_path(path: &str) -> Response<Body> {
    let raw = path.trim_start_matches('/');
    if raw.contains("..") {
        return (StatusCode::BAD_REQUEST, "bad path").into_response();
    }

    let wanted = if raw.is_empty() { "index.html" } else { raw };
    let bytes = crate::http::embedded_ui::read_asset_bytes(wanted)
        .or_else(|| crate::http::embedded_ui::read_asset_bytes("index.html"));
    let Some(bytes) = bytes else {
        return (StatusCode::NOT_FOUND, "missing ui").into_response();
    };
    let mime = crate::http::embedded_ui::guess_mime(wanted);

    let mut out = Response::new(Body::from(bytes));
    out.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&mime)
            .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
    );
    out
}

async fn proxy_handler(
    State(state): State<ProxyState>,
    request: HttpRequest<Body>,
) -> Response<Body> {
    let (parts, body) = request.into_parts();
    let target_url = build_target_url(&state.backend_base_url, &parts.uri);
    let max_body_bytes = crate::gateway::front_proxy_max_body_bytes();

    if let Some(content_length) = parts
        .headers
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
    {
        if content_length > max_body_bytes as u64 {
            return text_response(
                StatusCode::PAYLOAD_TOO_LARGE,
                format!("request body too large: content-length={content_length}"),
            );
        }
    }

    let outbound_headers = filter_request_headers(&parts.headers);
    let body_bytes = match to_bytes(body, max_body_bytes).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return text_response(
                StatusCode::PAYLOAD_TOO_LARGE,
                format!("request body too large: content-length>{max_body_bytes}"),
            );
        }
    };

    let mut builder = state.client.request(parts.method, target_url);
    builder = builder.headers(outbound_headers);
    builder = builder.body(body_bytes);

    let upstream = match builder.send().await {
        Ok(response) => response,
        Err(err) => {
            return text_response(
                StatusCode::BAD_GATEWAY,
                format!("backend proxy error: {err}"),
            );
        }
    };

    let response_builder = merge_upstream_headers(
        Response::builder().status(upstream.status()),
        upstream.headers(),
    );

    match response_builder.body(Body::from_stream(upstream.bytes_stream())) {
        Ok(response) => response,
        Err(err) => text_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("build response failed: {err}"),
        ),
    }
}

pub(crate) fn run_front_proxy(addr: &str, backend_addr: &str) -> io::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;

    runtime.block_on(async move {
        let client = Client::builder()
            .build()
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
        let missing_ui_html = Arc::new(builtin_missing_ui_html("apps/dist/index.html missing"));
        let state = ProxyState {
            backend_base_url: build_backend_base_url(backend_addr),
            client,
            rpc_token: crate::rpc_auth_token().to_string(),
            missing_ui_html,
        };
        let mut app = Router::new()
            .route("/api/rpc", post(rpc_proxy))
            .route("/v1/{*path}", any(gateway_proxy));
        if crate::http::embedded_ui::has_embedded_ui() {
            app = app
                .route("/", get(serve_embedded_index))
                .route("/{*path}", get(serve_embedded_asset));
        } else {
            app = app
                .route("/", get(serve_missing_ui))
                .route("/{*path}", get(serve_missing_ui));
        }
        let app = app.fallback(any(proxy_handler)).with_state(state);
        run_proxy_server(addr, app).await
    })
}

#[cfg(test)]
#[path = "tests/proxy_runtime_tests.rs"]
mod tests;
