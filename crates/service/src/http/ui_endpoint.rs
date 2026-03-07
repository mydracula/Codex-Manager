use tiny_http::{Header, Method, Request, Response, StatusCode};

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

fn respond_bytes(status: StatusCode, content_type: &str, bytes: Vec<u8>) -> Response<std::io::Cursor<Vec<u8>>> {
    let mut response = Response::from_data(bytes).with_status_code(status);
    if let Ok(header) = Header::from_bytes(b"Content-Type", content_type.as_bytes()) {
        response = response.with_header(header);
    }
    response
}

fn serve_embedded_path(path: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let raw = path.trim_start_matches('/');
    if raw.contains("..") {
        return respond_bytes(StatusCode(400), "text/plain; charset=utf-8", b"bad path".to_vec());
    }

    let wanted = if raw.is_empty() { "index.html" } else { raw };
    let bytes = crate::http::embedded_ui::read_asset_bytes(wanted)
        .or_else(|| crate::http::embedded_ui::read_asset_bytes("index.html"));
    let Some(bytes) = bytes else {
        let html = builtin_missing_ui_html("apps/dist/index.html missing");
        return respond_bytes(StatusCode(404), "text/html; charset=utf-8", html.into_bytes());
    };
    let mime = crate::http::embedded_ui::guess_mime(wanted);
    respond_bytes(StatusCode(200), &mime, bytes.to_vec())
}

pub(crate) fn handle_ui(request: Request) {
    if request.method() != &Method::Get {
        let _ = request.respond(Response::from_string("method not allowed").with_status_code(405));
        return;
    }

    let path = request
        .url()
        .split('?')
        .next()
        .unwrap_or(request.url())
        .to_string();

    if crate::http::embedded_ui::has_embedded_ui() {
        let _ = request.respond(serve_embedded_path(&path));
        return;
    }

    let html = builtin_missing_ui_html("apps/dist/index.html missing");
    let _ = request.respond(respond_bytes(
        StatusCode(200),
        "text/html; charset=utf-8",
        html.into_bytes(),
    ));
}
