use tiny_http::Request;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BackendRoute {
    Rpc,
    AuthCallback,
    Metrics,
    Ui,
    Gateway,
}

fn should_serve_ui(method: &str, path: &str) -> bool {
    if method != "GET" {
        return false;
    }
    if path == "/" {
        return true;
    }
    !path.starts_with("/v1/")
        && !path.starts_with("/auth/")
        && !path.starts_with("/api/")
        && path != "/rpc"
        && path != "/metrics"
        && path != "/health"
}

pub(crate) fn resolve_backend_route(method: &str, path: &str) -> BackendRoute {
    let path = path.split('?').next().unwrap_or(path);
    if method == "POST" && (path == "/rpc" || path == "/api/rpc") {
        return BackendRoute::Rpc;
    }
    if method == "GET" && path.starts_with("/auth/callback") {
        return BackendRoute::AuthCallback;
    }
    if method == "GET" && path == "/metrics" {
        return BackendRoute::Metrics;
    }
    if should_serve_ui(method, path) {
        return BackendRoute::Ui;
    }
    BackendRoute::Gateway
}

pub(crate) fn handle_backend_request(request: Request) {
    let route = resolve_backend_route(request.method().as_str(), request.url());
    match route {
        BackendRoute::Rpc => crate::http::rpc_endpoint::handle_rpc(request),
        BackendRoute::AuthCallback => crate::http::callback_endpoint::handle_callback(request),
        BackendRoute::Metrics => crate::http::gateway_endpoint::handle_metrics(request),
        BackendRoute::Ui => crate::http::ui_endpoint::handle_ui(request),
        BackendRoute::Gateway => crate::http::gateway_endpoint::handle_gateway(request),
    }
}

#[cfg(test)]
#[path = "tests/backend_router_tests.rs"]
mod tests;
