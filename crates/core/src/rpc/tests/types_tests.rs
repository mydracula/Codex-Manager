use super::{AccountSummary, RequestLogSummary};

#[test]
fn account_summary_serialization_matches_compact_contract() {
    let summary = AccountSummary {
        id: "acc-1".to_string(),
        label: "主账号".to_string(),
        group_name: Some("TEAM".to_string()),
        sort: 10,
    };

    let value = serde_json::to_value(summary).expect("serialize account summary");
    let obj = value.as_object().expect("account summary object");

    for key in ["id", "label", "groupName", "sort"] {
        assert!(obj.contains_key(key), "missing key: {key}");
    }

    for key in [
        "workspaceId",
        "workspaceName",
        "note",
        "tags",
        "status",
        "updatedAt",
    ] {
        assert!(!obj.contains_key(key), "unexpected key: {key}");
    }
}

#[test]
fn request_log_summary_serialization_includes_trace_route_fields() {
    let summary = RequestLogSummary {
        trace_id: Some("trc_1".to_string()),
        key_id: Some("gk_1".to_string()),
        account_id: Some("acc_1".to_string()),
        request_path: "/v1/responses".to_string(),
        original_path: Some("/v1/chat/completions".to_string()),
        adapted_path: Some("/v1/responses".to_string()),
        method: "POST".to_string(),
        model: Some("gpt-5.3-codex".to_string()),
        reasoning_effort: Some("high".to_string()),
        response_adapter: Some("OpenAIChatCompletionsJson".to_string()),
        upstream_url: Some("https://api.openai.com/v1".to_string()),
        status_code: Some(502),
        input_tokens: Some(10),
        cached_input_tokens: Some(0),
        output_tokens: Some(3),
        total_tokens: Some(13),
        reasoning_output_tokens: Some(1),
        estimated_cost_usd: Some(0.12),
        error: Some("internal_error".to_string()),
        created_at: 1,
    };

    let value = serde_json::to_value(summary).expect("serialize request log summary");
    let obj = value.as_object().expect("request log summary object");
    for key in [
        "traceId",
        "originalPath",
        "adaptedPath",
        "responseAdapter",
        "requestPath",
        "upstreamUrl",
    ] {
        assert!(obj.contains_key(key), "missing key: {key}");
    }
}
