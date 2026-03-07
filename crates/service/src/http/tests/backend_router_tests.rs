use super::{resolve_backend_route, BackendRoute};

#[test]
fn resolves_rpc_route() {
    assert_eq!(resolve_backend_route("POST", "/rpc"), BackendRoute::Rpc);
    assert_eq!(resolve_backend_route("POST", "/api/rpc"), BackendRoute::Rpc);
}

#[test]
fn resolves_auth_callback_route() {
    assert_eq!(
        resolve_backend_route("GET", "/auth/callback?code=123"),
        BackendRoute::AuthCallback
    );
}

#[test]
fn resolves_metrics_route() {
    assert_eq!(
        resolve_backend_route("GET", "/metrics"),
        BackendRoute::Metrics
    );
}

#[test]
fn resolves_ui_route() {
    assert_eq!(resolve_backend_route("GET", "/"), BackendRoute::Ui);
    assert_eq!(resolve_backend_route("GET", "/assets/index.js"), BackendRoute::Ui);
}

#[test]
fn falls_back_to_gateway_route() {
    assert_eq!(
        resolve_backend_route("POST", "/v1/responses"),
        BackendRoute::Gateway
    );
}
