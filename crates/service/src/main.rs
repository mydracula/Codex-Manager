fn main() {
    codexmanager_service::portable::bootstrap_current_process();
    let raw_addr = std::env::var("CODEXMANAGER_SERVICE_ADDR")
        .or_else(|_| std::env::var("PORT"))
        .unwrap_or_else(|_| codexmanager_service::default_listener_bind_addr());
    let addr = codexmanager_service::listener_bind_addr(&raw_addr);
    println!("codexmanager-service listening on {addr}");
    if let Err(err) = codexmanager_service::start_server(&addr) {
        eprintln!("service stopped: {err}");
        std::process::exit(1);
    }
}
