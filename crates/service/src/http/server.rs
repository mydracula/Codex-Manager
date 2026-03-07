use crate::http::backend_runtime::{serve_http, start_backend_server, wake_backend_shutdown};
use crate::http::proxy_runtime::run_front_proxy;

fn should_use_direct_listener(addr: &str) -> bool {
    !(addr.starts_with("localhost:")
        || addr.starts_with("127.0.0.1:")
        || addr.starts_with("[::1]:")
        || addr.starts_with("::1:"))
}

pub fn start_http(addr: &str) -> std::io::Result<()> {
    if should_use_direct_listener(addr) {
        return serve_http(addr);
    }

    let backend = start_backend_server()?;
    let result = run_front_proxy(addr, &backend.addr);
    wake_backend_shutdown(&backend.addr);
    let _ = backend.join.join();
    result
}
