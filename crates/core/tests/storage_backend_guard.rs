use codexmanager_core::storage::{
    now_ts, Event, LoginSession, RequestLog, RequestTokenStat, Storage, StorageError,
};

#[test]
fn storage_open_accepts_postgres_url_for_backend_routing() {
    Storage::open("postgres://user:pass@localhost/codexmanager")
        .expect("postgres locator should route into postgres backend");
}

#[test]
fn storage_open_reports_mysql_backend_as_not_implemented() {
    let err = Storage::open("mysql://user:pass@localhost/codexmanager")
        .expect_err("mysql should not silently fall back to sqlite");
    assert!(matches!(err, StorageError::BackendNotImplemented(_)));
    assert!(err.to_string().contains("mysql"));
}

#[test]
fn postgres_backend_methods_fail_with_connection_error_instead_of_sqlite_panic() {
    let storage = Storage::open("postgres://user:pass@127.0.0.1:1/codexmanager")
        .expect("postgres locator should route into postgres backend");

    let event_err = storage
        .insert_event(&Event {
            account_id: None,
            event_type: "info".to_string(),
            message: "hello".to_string(),
            created_at: now_ts(),
        })
        .expect_err("postgres event write should attempt pg path");
    assert!(!matches!(event_err, StorageError::Sqlite(_)));

    let stat_err = storage
        .insert_request_token_stat(&RequestTokenStat {
            request_log_id: 1,
            key_id: None,
            account_id: None,
            model: None,
            input_tokens: Some(1),
            cached_input_tokens: None,
            output_tokens: None,
            total_tokens: Some(1),
            reasoning_output_tokens: None,
            estimated_cost_usd: Some(0.1),
            created_at: now_ts(),
        })
        .expect_err("postgres token stat write should attempt pg path");
    assert!(!matches!(stat_err, StorageError::Sqlite(_)));

    let request_log_err = storage
        .insert_request_log(&RequestLog {
            trace_id: Some("trace-pg".to_string()),
            key_id: None,
            account_id: None,
            request_path: "/v1/responses".to_string(),
            original_path: None,
            adapted_path: None,
            method: "POST".to_string(),
            model: None,
            reasoning_effort: None,
            response_adapter: None,
            upstream_url: None,
            status_code: Some(200),
            input_tokens: None,
            cached_input_tokens: None,
            output_tokens: None,
            total_tokens: None,
            reasoning_output_tokens: None,
            estimated_cost_usd: None,
            error: None,
            created_at: now_ts(),
        })
        .expect_err("postgres request log write should attempt pg path");
    assert!(!matches!(request_log_err, StorageError::Sqlite(_)));

    let login_session_err = storage
        .insert_login_session(&LoginSession {
            login_id: "login-pg".to_string(),
            code_verifier: "verifier".to_string(),
            state: "state".to_string(),
            status: "pending".to_string(),
            error: None,
            note: None,
            tags: None,
            group_name: None,
            created_at: now_ts(),
            updated_at: now_ts(),
        })
        .expect_err("postgres login session write should attempt pg path");
    assert!(!matches!(login_session_err, StorageError::Sqlite(_)));

    let get_login_session_err = storage
        .get_login_session("login-pg")
        .expect_err("postgres login session read should attempt pg path");
    assert!(!matches!(get_login_session_err, StorageError::Sqlite(_)));

    let update_login_session_err = storage
        .update_login_session_status("login-pg", "done", None)
        .expect_err("postgres login session update should attempt pg path");
    assert!(!matches!(update_login_session_err, StorageError::Sqlite(_)));

    let list_request_logs_err = storage
        .list_request_logs(Some("method:POST"), 10)
        .expect_err("postgres request log list should attempt pg path");
    assert!(!matches!(list_request_logs_err, StorageError::Sqlite(_)));

    let list_app_settings_err = storage
        .list_app_settings()
        .expect_err("postgres app settings list should attempt pg path");
    assert!(!matches!(list_app_settings_err, StorageError::Sqlite(_)));

    let get_app_setting_err = storage
        .get_app_setting("theme")
        .expect_err("postgres app settings read should attempt pg path");
    assert!(!matches!(get_app_setting_err, StorageError::Sqlite(_)));

    let set_app_setting_err = storage
        .set_app_setting("theme", "dark", now_ts())
        .expect_err("postgres app settings write should attempt pg path");
    assert!(!matches!(set_app_setting_err, StorageError::Sqlite(_)));

    let delete_app_setting_err = storage
        .delete_app_setting("theme")
        .expect_err("postgres app settings delete should attempt pg path");
    assert!(!matches!(delete_app_setting_err, StorageError::Sqlite(_)));

    let upsert_model_options_err = storage
        .upsert_model_options_cache("global", "[]", now_ts())
        .expect_err("postgres model options write should attempt pg path");
    assert!(!matches!(upsert_model_options_err, StorageError::Sqlite(_)));

    let get_model_options_err = storage
        .get_model_options_cache("global")
        .expect_err("postgres model options read should attempt pg path");
    assert!(!matches!(get_model_options_err, StorageError::Sqlite(_)));
}
