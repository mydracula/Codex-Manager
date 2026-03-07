use codexmanager_core::rpc::types::{DashboardHighlightAccount, DashboardHighlightsResult};
use codexmanager_core::storage::{Account, RequestLog, UsageSnapshotRecord};

use crate::storage_helpers::open_storage;
use crate::usage_read::usage_snapshot_result_from_record;

pub(crate) fn read_dashboard_highlights() -> Result<DashboardHighlightsResult, String> {
    let storage = open_storage().ok_or_else(|| "open storage failed".to_string())?;
    let accounts = storage
        .list_accounts_filtered(None, None)
        .map_err(|err| format!("list accounts failed: {err}"))?;
    let usage_items = storage
        .latest_usage_snapshots_by_account()
        .map_err(|err| format!("list usage snapshots failed: {err}"))?;
    let request_logs = storage
        .list_request_logs(None, 40)
        .map_err(|err| format!("list request logs failed: {err}"))?;

    let usage_by_account = usage_items
        .into_iter()
        .map(|item| (item.account_id.clone(), item))
        .collect::<std::collections::HashMap<_, _>>();

    let current = pick_current_account(
        &accounts,
        &usage_by_account,
        &request_logs,
        crate::gateway::manual_preferred_account(),
    );
    let (primary_recommendation, secondary_recommendation) =
        pick_recommendations(&accounts, &usage_by_account);

    Ok(DashboardHighlightsResult {
        current,
        primary_recommendation,
        secondary_recommendation,
    })
}

fn pick_current_account(
    accounts: &[Account],
    usage_by_account: &std::collections::HashMap<String, UsageSnapshotRecord>,
    request_logs: &[RequestLog],
    manual_preferred_account_id: Option<String>,
) -> Option<DashboardHighlightAccount> {
    let preferred_id = manual_preferred_account_id.unwrap_or_default();
    if !preferred_id.is_empty() {
        if let Some(account) = accounts.iter().find(|item| item.id == preferred_id) {
            if can_participate_in_routing(account, usage_by_account.get(&account.id)) {
                return Some(to_highlight_account(account, usage_by_account.get(&account.id)));
            }
        }
    }

    let latest_request_account_id = request_logs
        .iter()
        .filter_map(|item| item.account_id.as_deref().map(str::to_string).map(|id| (id, item.created_at)))
        .max_by_key(|(_, created_at)| *created_at)
        .map(|(account_id, _)| account_id);
    if let Some(account_id) = latest_request_account_id {
        if let Some(account) = accounts.iter().find(|item| item.id == account_id) {
            if can_participate_in_routing(account, usage_by_account.get(&account.id)) {
                return Some(to_highlight_account(account, usage_by_account.get(&account.id)));
            }
        }
    }

    accounts
        .iter()
        .find(|account| can_participate_in_routing(account, usage_by_account.get(&account.id)))
        .map(|account| to_highlight_account(account, usage_by_account.get(&account.id)))
        .or_else(|| {
            accounts
                .first()
                .map(|account| to_highlight_account(account, usage_by_account.get(&account.id)))
        })
}

fn pick_recommendations(
    accounts: &[Account],
    usage_by_account: &std::collections::HashMap<String, UsageSnapshotRecord>,
) -> (
    Option<DashboardHighlightAccount>,
    Option<DashboardHighlightAccount>,
) {
    let mut primary_pick: Option<(&Account, f64)> = None;
    let mut secondary_pick: Option<(&Account, f64)> = None;

    for account in accounts {
        let usage = usage_by_account.get(&account.id);
        if !can_participate_in_routing(account, usage) {
            continue;
        }
        if let Some(primary_remain) = remaining_percent(usage.and_then(|item| item.used_percent)) {
            if primary_pick
                .map(|(_, remain)| primary_remain > remain)
                .unwrap_or(true)
            {
                primary_pick = Some((account, primary_remain));
            }
        }
        if let Some(secondary_remain) = remaining_percent(usage.and_then(|item| item.secondary_used_percent)) {
            if secondary_pick
                .map(|(_, remain)| secondary_remain > remain)
                .unwrap_or(true)
            {
                secondary_pick = Some((account, secondary_remain));
            }
        }
    }

    (
        primary_pick.map(|(account, _)| to_highlight_account(account, usage_by_account.get(&account.id))),
        secondary_pick.map(|(account, _)| to_highlight_account(account, usage_by_account.get(&account.id))),
    )
}

fn to_highlight_account(
    account: &Account,
    usage: Option<&UsageSnapshotRecord>,
) -> DashboardHighlightAccount {
    let (status_level, status_text) = classify_availability(account, usage);
    DashboardHighlightAccount {
        id: account.id.clone(),
        label: account.label.clone(),
        status_level: status_level.to_string(),
        status_text: status_text.to_string(),
        usage: usage.cloned().map(usage_snapshot_result_from_record),
    }
}

fn can_participate_in_routing(account: &Account, usage: Option<&UsageSnapshotRecord>) -> bool {
    let (status_level, _) = classify_availability(account, usage);
    status_level != "warn" && status_level != "bad"
}

fn classify_availability(
    account: &Account,
    usage: Option<&UsageSnapshotRecord>,
) -> (&'static str, &'static str) {
    if account.status.trim().eq_ignore_ascii_case("inactive") {
        return ("bad", "不可用");
    }
    let Some(usage) = usage else {
        return ("unknown", "未知");
    };
    match usage_snapshot_result_from_record(usage.clone())
        .availability_status
        .unwrap_or_default()
        .as_str()
    {
        "available" => ("ok", "可用"),
        "primary_window_available_only" => ("ok", "单窗口可用"),
        "unavailable" => ("bad", "不可用"),
        _ => ("unknown", "未知"),
    }
}

fn remaining_percent(value: Option<f64>) -> Option<f64> {
    value.map(|used| (100.0 - used).clamp(0.0, 100.0))
}
