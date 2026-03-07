use std::io::Read;

use tiny_http::Request;
use tiny_http::Response;
use url::Url;

fn rpc_response_failed(resp: &codexmanager_core::rpc::types::JsonRpcResponse) -> bool {
    if resp.result.get("error").is_some() {
        return true;
    }
    matches!(
        resp.result.get("ok").and_then(|value| value.as_bool()),
        Some(false)
    )
}

fn get_header_value<'a>(request: &'a Request, name: &str) -> Option<&'a str> {
    request
        .headers()
        .iter()
        .find(|header| header.field.as_str().as_str().eq_ignore_ascii_case(name))
        .map(|header| header.value.as_str().trim())
        .filter(|value| !value.is_empty())
}

fn is_json_content_type(request: &Request) -> bool {
    get_header_value(request, "Content-Type")
        .and_then(|value| value.split(';').next())
        .map(|value| value.trim().eq_ignore_ascii_case("application/json"))
        .unwrap_or(false)
}

fn is_loopback_origin(origin: &str) -> bool {
    let Ok(url) = Url::parse(origin) else {
        return false;
    };
    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }
    matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "::1"))
}

fn same_origin_matches_host(origin: &str, host: &str) -> bool {
    let Ok(url) = Url::parse(origin) else {
        return false;
    };
    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }
    let Some(origin_host) = url.host_str() else {
        return false;
    };
    let origin_port = url.port_or_known_default();
    let host = host.trim();
    if host.is_empty() {
        return false;
    }
    let mut parts = host.rsplitn(2, ':');
    let host_tail = parts.next().unwrap_or_default();
    let host_head = parts.next();
    let (host_name, host_port) = match host_head {
        Some(name) if !name.is_empty() && host_tail.chars().all(|ch| ch.is_ascii_digit()) => {
            (name.trim_matches(['[', ']']), host_tail.parse::<u16>().ok())
        }
        _ => (host.trim_matches(['[', ']']), None),
    };
    origin_host.eq_ignore_ascii_case(host_name) && origin_port == host_port.or(url.port_or_known_default())
}

pub fn handle_rpc(mut request: Request) {
    let mut rpc_metrics_guard = crate::gateway::begin_rpc_request();
    let request_path = request.url().split('?').next().unwrap_or(request.url()).to_string();
    let same_origin_api_rpc = request_path == "/api/rpc";
    if request.method().as_str() != "POST" {
        let _ = request.respond(Response::from_string("{}").with_status_code(405));
        return;
    }
    if !is_json_content_type(&request) {
        let _ = request.respond(Response::from_string("{}").with_status_code(415));
        return;
    }

    let request_host = get_header_value(&request, "Host").map(|value| value.to_string());
    let origin = get_header_value(&request, "Origin").map(|value| value.to_string());
    let fetch_site = get_header_value(&request, "Sec-Fetch-Site").map(|value| value.to_string());

    let allow_same_origin_without_token = same_origin_api_rpc
        && origin
            .as_deref()
            .zip(request_host.as_deref())
            .is_some_and(|(origin, host)| same_origin_matches_host(origin, host));

    match get_header_value(&request, "X-CodexManager-Rpc-Token") {
        Some(token) => {
            if !crate::rpc_auth_token_matches(token) {
                let _ = request.respond(Response::from_string("{}").with_status_code(401));
                return;
            }
        }
        None if allow_same_origin_without_token => {}
        None => {
            let _ = request.respond(Response::from_string("{}").with_status_code(401));
            return;
        }
    }

    if let Some(fetch_site) = fetch_site.as_deref() {
        if fetch_site.eq_ignore_ascii_case("cross-site") {
            let _ = request.respond(Response::from_string("{}").with_status_code(403));
            return;
        }
    }
    if let Some(origin) = origin.as_deref() {
        let allowed_origin = is_loopback_origin(origin)
            || request_host
                .as_deref()
                .is_some_and(|host| same_origin_matches_host(origin, host));
        if !allowed_origin {
            let _ = request.respond(Response::from_string("{}").with_status_code(403));
            return;
        }
    }

    let max_body_bytes = crate::gateway::front_proxy_max_body_bytes();
    let mut body_bytes = Vec::new();
    let mut reader = request.as_reader().take(max_body_bytes as u64 + 1);
    if reader.read_to_end(&mut body_bytes).is_err() {
        let _ = request.respond(Response::from_string("{}").with_status_code(400));
        return;
    }
    if body_bytes.len() > max_body_bytes {
        let _ = request.respond(Response::from_string("{}").with_status_code(413));
        return;
    }
    if body_bytes.iter().all(|value| value.is_ascii_whitespace()) {
        let _ = request.respond(Response::from_string("{}").with_status_code(400));
        return;
    }
    let body = match String::from_utf8(body_bytes) {
        Ok(value) => value,
        Err(_) => {
            let _ = request.respond(Response::from_string("{}").with_status_code(400));
            return;
        }
    };

    let req: codexmanager_core::rpc::types::JsonRpcRequest = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(_) => {
            let _ = request.respond(Response::from_string("{}").with_status_code(400));
            return;
        }
    };
    let resp = crate::handle_request(req);
    if !rpc_response_failed(&resp) {
        rpc_metrics_guard.mark_success();
    }
    let json = serde_json::to_string(&resp).unwrap_or_else(|_| "{}".to_string());
    let _ = request.respond(Response::from_string(json));
}
