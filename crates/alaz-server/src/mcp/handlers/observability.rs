use alaz_db::repos::{AlertHistoryRepo, AlertRuleRepo, ErrorGroupRepo, StructuredLogRepo};

use super::super::params::*;
use crate::state::AppState;

// --- Logs ---

pub(crate) async fn logs_query(
    state: &AppState,
    params: LogsQueryParams,
) -> Result<String, String> {
    let limit = params.limit.unwrap_or(50);
    let since_secs = params.since_secs.or(Some(3600));

    let logs = StructuredLogRepo::query(
        &state.pool,
        params.level.as_deref(),
        params.target.as_deref(),
        params.search.as_deref(),
        since_secs,
        limit,
    )
    .await
    .map_err(|e| format!("logs query failed: {e}"))?;

    if logs.is_empty() {
        return Ok("No logs matching filters.".into());
    }

    let mut output = format!("## Logs ({} entries)\n\n", logs.len());
    output.push_str("| Time | Level | Target | Message |\n");
    output.push_str("|------|-------|--------|----------|\n");

    for log in &logs {
        let msg = if log.message.len() > 100 {
            format!("{}...", &log.message[..100])
        } else {
            log.message.clone()
        };
        output.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            log.timestamp.format("%H:%M:%S"),
            log.level,
            log.target.rsplit("::").next().unwrap_or(&log.target),
            msg.replace('|', "\\|"),
        ));
    }

    Ok(output)
}

pub(crate) async fn logs_stats(state: &AppState, params: LogStatsParams) -> Result<String, String> {
    let since_secs = params.since_secs.unwrap_or(3600);
    let stats = StructuredLogRepo::stats_by_level(&state.pool, since_secs)
        .await
        .map_err(|e| format!("log stats failed: {e}"))?;

    if stats.is_empty() {
        return Ok(format!("No logs in the last {since_secs}s."));
    }

    let mut output = format!(
        "## Log Stats (last {}s)\n\n| Level | Count |\n|-------|-------|\n",
        since_secs
    );
    for s in &stats {
        output.push_str(&format!("| {} | {} |\n", s.level, s.count));
    }
    Ok(output)
}

// --- Error Groups ---

pub(crate) async fn error_groups_list(
    state: &AppState,
    params: ErrorGroupsParams,
) -> Result<String, String> {
    let limit = params.limit.unwrap_or(20);
    let groups = ErrorGroupRepo::list(&state.pool, params.status.as_deref(), limit)
        .await
        .map_err(|e| format!("error groups list failed: {e}"))?;

    if groups.is_empty() {
        return Ok("No error groups.".into());
    }

    let mut output = format!("## Error Groups ({} entries)\n\n", groups.len());
    output.push_str("| ID | Count | Status | Target | Title |\n");
    output.push_str("|----|-------|--------|--------|-------|\n");

    for g in &groups {
        let id_short: String = g.id.chars().take(8).collect();
        let title = if g.title.len() > 60 {
            format!("{}...", &g.title[..60])
        } else {
            g.title.clone()
        };
        output.push_str(&format!(
            "| `{}` | {} | {} | {} | {} |\n",
            id_short,
            g.event_count,
            g.status,
            g.target.rsplit("::").next().unwrap_or(&g.target),
            title.replace('|', "\\|"),
        ));
    }

    Ok(output)
}

pub(crate) async fn error_group_detail(
    state: &AppState,
    params: ErrorGroupDetailParams,
) -> Result<String, String> {
    let group = ErrorGroupRepo::get(&state.pool, &params.id)
        .await
        .map_err(|e| format!("error group not found: {e}"))?;

    let mut output = format!(
        "## Error Group: {}\n\n\
        **Status**: {}\n\
        **Target**: {}\n\
        **First seen**: {}\n\
        **Last seen**: {}\n\
        **Event count**: {}\n\
        **Fingerprint**: `{}`\n\n\
        **Title**:\n> {}\n\n",
        &group.id[..8],
        group.status,
        group.target,
        group.first_seen.format("%Y-%m-%d %H:%M:%S"),
        group.last_seen.format("%Y-%m-%d %H:%M:%S"),
        group.event_count,
        group.fingerprint,
        group.title,
    );

    if let Some(notes) = &group.resolution_notes {
        output.push_str(&format!("**Resolution notes**: {notes}\n"));
    }

    Ok(output)
}

pub(crate) async fn resolve_error_group(
    state: &AppState,
    params: ResolveErrorGroupParams,
) -> Result<String, String> {
    ErrorGroupRepo::resolve(&state.pool, &params.id, params.notes.as_deref())
        .await
        .map_err(|e| format!("resolve failed: {e}"))?;
    Ok(format!(
        "Error group `{}` marked as resolved.",
        &params.id[..8]
    ))
}

// --- Alert Rules ---

pub(crate) async fn create_alert_rule(
    state: &AppState,
    params: CreateAlertRuleParams,
) -> Result<String, String> {
    let rule = AlertRuleRepo::create(
        &state.pool,
        &params.name,
        params.description.as_deref(),
        &params.condition_type,
        params.threshold,
        params.window_secs.unwrap_or(300),
        params.filter_level.as_deref(),
        params.filter_target.as_deref(),
        params.filter_pattern.as_deref(),
    )
    .await
    .map_err(|e| format!("create alert failed: {e}"))?;

    Ok(format!(
        "Alert rule created: **{}**\nID: `{}`\nCondition: {} (threshold: {}, window: {}s)",
        rule.name, rule.id, rule.condition_type, rule.threshold, rule.window_secs
    ))
}

pub(crate) async fn list_alert_rules(
    state: &AppState,
    _params: ListAlertRulesParams,
) -> Result<String, String> {
    let rules = AlertRuleRepo::list_all(&state.pool)
        .await
        .map_err(|e| format!("list alerts failed: {e}"))?;

    if rules.is_empty() {
        return Ok("No alert rules configured.".into());
    }

    let mut output = format!("## Alert Rules ({})\n\n", rules.len());
    output.push_str(
        "| Enabled | Name | Condition | Threshold | Window | Triggers | Last Triggered |\n",
    );
    output.push_str(
        "|---------|------|-----------|-----------|--------|----------|----------------|\n",
    );

    for r in &rules {
        let last = r
            .last_triggered_at
            .map(|t| t.format("%m-%d %H:%M").to_string())
            .unwrap_or_else(|| "-".into());
        output.push_str(&format!(
            "| {} | {} | {} | {} | {}s | {} | {} |\n",
            if r.enabled { "✓" } else { "✗" },
            r.name,
            r.condition_type,
            r.threshold,
            r.window_secs,
            r.trigger_count,
            last,
        ));
    }

    Ok(output)
}

pub(crate) async fn delete_alert_rule(
    state: &AppState,
    params: DeleteAlertRuleParams,
) -> Result<String, String> {
    AlertRuleRepo::delete(&state.pool, &params.id)
        .await
        .map_err(|e| format!("delete failed: {e}"))?;
    Ok(format!("Alert rule `{}` deleted.", params.id))
}

pub(crate) async fn alert_history(
    state: &AppState,
    params: AlertHistoryParams,
) -> Result<String, String> {
    let limit = params.limit.unwrap_or(20);
    let entries = AlertHistoryRepo::recent(&state.pool, limit)
        .await
        .map_err(|e| format!("alert history failed: {e}"))?;

    if entries.is_empty() {
        return Ok("No alerts have triggered yet.".into());
    }

    let mut output = format!("## Alert History ({} entries)\n\n", entries.len());
    for e in &entries {
        output.push_str(&format!(
            "- **{}**: rule `{}` triggered with {} matches\n",
            e.triggered_at.format("%Y-%m-%d %H:%M:%S"),
            &e.alert_rule_id[..8],
            e.matched_count,
        ));
    }
    Ok(output)
}
