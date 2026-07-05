mod operator_dashboard_active_freshness_prefers_live_activity_source;
mod operator_dashboard_header_shows_endpoint_and_snapshot_freshness;
mod operator_dashboard_run_activity_consumes_server_presentation;
mod operator_dashboard_uses_shared_protocol_activity_summary;
mod operator_dashboard_uses_websocket_without_http_state_fallback;

fn assert_dashboard_freshness_source_contract(response: &str) {
	assert!(response.contains("function currentLaneFreshness(run)"));
	assert!(response.contains("source: \"last_run_activity_at\""));
	assert!(response.contains("source: \"none\""));
	assert!(!response.contains("source: \"updated_at\""));
	assert!(response.contains("function formatRelativeTimestamp(value)"));
	assert!(response.contains("return \"0s\";"));
	assert!(response.contains("return `${seconds}s`;"));
	assert!(response.contains("return `${minutes}m`;"));
	assert!(response.contains("return `${hours}h`;"));
	assert!(response.contains("return `${days}d`;"));
	assert!(response.contains("sourceLabel: \"live activity\""));
	assert!(response.contains("sourceLabel: \"protocol activity\""));
	assert!(
		response.contains("facts.push([\"lane idle\", formatDuration(run.idle_for_seconds)]);")
	);
	assert!(
		response.contains(
			"facts.push([\"agent idle\", formatDuration(run.protocol_idle_for_seconds)]);"
		)
	);
	assert!(response.contains("facts.push([\"focus\", detailLabel(focus)]);"));
}

fn assert_dashboard_lifecycle_activity_contract(response: &str) {
	assert!(
		response.contains(
			"function currentLaneLifecycleMetrics(run, summary = childAgentActivity(run))"
		)
	);
	assert!(
		response
			.contains("function lifecycleMetricFacts(metrics, { includeAttempts = false } = {})")
	);
	assert!(response.contains(
		"facts.push([\"run phase\", displayToken(run.run_phase || run.phase || run.status)]);"
	));
	assert!(
		!response
			.contains("facts.push([\"current operation\", displayToken(run.current_operation)]);")
	);
	assert!(
		!response
			.contains("facts.push([\"active goal phase\", displayToken(run.active_goal_phase)]);")
	);
	assert!(!response.contains(
		"facts.push([\"public progress phase\", displayToken(run.public_progress_phase)]);"
	));
	assert!(response.contains("function lifecycleRecoveryDebugSummary(metrics)"));
	assert!(response.contains("function lifecycleEvidenceDebugSummary(metrics)"));
	assert!(response.contains("${field(\"Lifecycle recovery\", lifecycleRecoveryDebugSummary(currentLaneLifecycleMetrics(run)))}"));
	assert!(response.contains("${field(\"Lifecycle evidence\", lifecycleEvidenceDebugSummary(currentLaneLifecycleMetrics(run)))}"));
	assert!(
		response.contains("${field(\"Run phase\", capturedValue(run.run_phase || run.phase))}")
	);
	assert!(
		response.contains("${field(\"Current operation\", capturedValue(run.current_operation))}")
	);
	assert!(
		response.contains("${field(\"Active goal phase\", capturedValue(run.active_goal_phase))}")
	);
	assert!(
		response.contains(
			"${field(\"Public progress phase\", capturedValue(run.public_progress_phase))}"
		)
	);
	assert!(response.contains("facts.push([\"tokens\", tokenSummary]);"));
	assert!(
		response.contains("facts.push([\"tools\", formatCompactCount(metrics.tool_call_count)]);")
	);
	assert!(response.contains("\"max output\","));
	assert!(response.contains("function childAgentContextRows(run, summary, lifecycle = currentLaneLifecycleMetrics(run, summary))"));
	assert!(response.contains("renderChildLifecycleOverview(lifecycle, contextFacts)"));
	assert!(response.contains("renderChildLifecyclePhaseTable(lifecycle.phases || [])"));
	assert!(
		!response
			.contains("rows.push(renderChildContextRow(\"Total\", totalFacts, \"is-total\"));")
	);
	assert!(
		response.contains(
			"<div class=\"child-context-group\" aria-label=\"Context lifecycle metrics\">"
		)
	);
	assert!(response.contains(".child-phase-table {\n\t\t\t\tdisplay: inline-grid;\n\t\t\t\tgrid-template-columns:\n\t\t\t\t\tmax-content"));
	assert!(!response.contains("function childAgentUsageFacts(summary)"));
	assert!(!response.contains("<span class=\"child-context-label\">Usage</span>"));
}

fn assert_dashboard_activity_display_regressions(response: &str) {
	assert!(response.contains("renderRunMetaFact(label, value)"));
	assert!(!response.contains("sourceLabel: \"Live Activity\""));
	assert!(
		!response.contains("facts.push([\"Lane Idle\", formatDuration(run.idle_for_seconds)]);")
	);
	assert!(
		!response.contains(
			"facts.push([\"Agent Idle\", formatDuration(run.protocol_idle_for_seconds)]);"
		)
	);
	assert!(!response.contains("${inlineStatusFact(label, value)}"));
	assert!(!response.contains("just now"));
	assert!(!response.contains("s ago"));
	assert!(!response.contains("m ago"));
	assert!(!response.contains("h ago"));
	assert!(!response.contains("d ago"));
	assert!(response.contains("function currentLaneTelemetryFacts(run)"));
	assert!(response.contains("function renderRunTelemetryMetaItems(run)"));
	assert!(
		response
			.contains("function renderRunMetaFact(label, value, valueClass = \"\", title = \"\")")
	);
	assert!(!response.contains("renderCurrentLaneActivityStrip(run)"));
	assert!(!response.contains("run-activity-strip"));
	assert!(!response.contains("function renderActiveTelemetryLine(run)"));
	assert!(!response.contains("activity-line"));
	assert!(
		response
			.contains("freshness.timestamp ? formatter(freshness.timestamp) : \"not captured\"")
	);
	assert!(!response.contains("Last ${freshness.sourceLabel}"));
	assert!(!response.contains("Latest ${freshness.sourceLabel}"));
	assert!(!response.contains("renderTimingStrip(run)"));
	assert!(!response.contains("currentLaneFreshnessSource(run)"));
	assert!(!response.contains("field(\"Freshness source\", currentLaneFreshnessSource(run))"));
	assert!(response.contains("field(\"Updated\", formatTimestamp(run.updated_at))"));
}
