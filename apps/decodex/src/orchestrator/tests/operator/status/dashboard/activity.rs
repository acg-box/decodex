use crate::orchestrator::tests::operator::status::dashboard;
#[test]
fn operator_dashboard_header_shows_endpoint_and_snapshot_freshness() {
	let response = dashboard::dashboard_response();

	assert!(response.contains("const DASHBOARD_WEBSOCKET_ENDPOINT = \"/dashboard/control\";"));
	assert!(!response.contains("SNAPSHOT_PUBLISHED_HEADER"));
	assert!(!response.contains("x-decodex-snapshot-unix-epoch"));
	assert!(!response.contains("function dashboardEndpointMeta(path)"));
	assert!(response.contains("function dashboardSocketUrl()"));
	assert!(!response.contains("function snapshotPublishedAtFromResponse(response)"));
	assert!(response.contains("function snapshotAgeSeconds(snapshotPublishedAt)"));
	assert!(response.contains("function snapshotFreshnessMeta("));
	assert!(response.contains("function topbarReadinessLabel(label)"));
	assert!(!response.contains("function topbarStreamLabel(label)"));
	assert!(response.contains("window.location.protocol === \"https:\" ? \"wss:\" : \"ws:\""));
	assert!(response.contains("<span>Transport</span>"));
	assert!(response.contains("topbarReadinessLabel(readiness.label)"));
	assert!(response.contains(
		"<span class=\"transport-meta\" data-kind=\"endpoint\" data-tone=\"${escapeHtml(stream.tone)}\""
	));
	assert!(response.contains("<span>Transport</span><strong>${renderValueLink(\"WebSocket\", dashboardSocketUrl(), \"transport-link\") || escapeHtml(dashboardSocketUrl())}</strong>"));
	assert!(!response.contains("topbarStreamLabel(stream.label)"));
	assert!(!response.contains("Poll fallback"));
	assert!(response.contains("<span class=\"transport-meta\" data-kind=\"snapshot\" data-tone=\"${escapeHtml(snapshotFreshness.tone)}\""));
	assert!(response.contains("<span>Snapshot</span>"));
	assert!(response.contains("case \"Snapshot ready\":"));
	assert!(response.contains("return \"Ready\";"));
	assert!(!response.contains("return \"Connected\";"));
	assert!(response.contains("label: \"Unavailable\""));
	assert!(response.contains("label: \"Pending\""));
	assert!(response.contains("WebSocket connected."));
	assert!(response.contains("const snapshotFreshnessRow = snapshotFreshness"));
	assert!(response.contains("return null;"));
	assert!(response.contains("const staleByAge = ageSeconds != null && ageSeconds >= 30;"));
	assert!(!response.contains("const staleByReadiness"));
	assert!(response.contains("data-tone=\"${escapeHtml(snapshotFreshness.tone)}\""));
	assert!(response.contains("Published ${formatTimestamp(snapshotPublishedAt)}"));
	assert!(response.contains("formatRelativeTimestamp(snapshotPublishedAt)"));
	assert!(response.contains("unixEpochSecondsToIso(payload.snapshotPublishedAtUnixEpoch)"));
	assert!(!response.contains("stateResult.value.snapshotPublishedAt"));
	assert!(!response.contains("readinessResponse"));
	assert!(!response.contains("body: \"ready\""));
	assert!(!response.contains("polling active"));
	assert!(response.contains(
		"renderHeader(snapshot, readiness, notices, snapshotPublishedAt, snapshotError)"
	));
	assert!(response.contains(".transport-meta"));
	assert!(response.contains("max-width: min(42vw, 320px);"));
	assert!(!response.contains("Auto-refresh"));
	assert!(!response.contains("Diagnostics"));
}

#[test]
fn operator_dashboard_active_freshness_prefers_live_activity_source() {
	let response = dashboard::dashboard_response();

	assert_dashboard_freshness_source_contract(&response);
	assert_dashboard_lifecycle_activity_contract(&response);
	assert_dashboard_activity_display_regressions(&response);
}

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

#[test]
fn operator_dashboard_uses_shared_protocol_activity_summary() {
	let response = dashboard::dashboard_response();

	assert!(response.contains("function protocolActivity(run)"));
	assert!(response.contains("function protocolActivityFocus(run)"));
	assert!(response.contains("function protocolActivityRecentSummary(run)"));
	assert!(response.contains("function protocolActivityDebugSummary(run)"));
	assert!(!response.contains("function normalizedProtocolRateLimitStatus(value)"));
	assert!(!response.contains("status.includes(\"/\") || status.includes(\" \")"));
	assert!(!response.contains("protocolActivityRateLimitDisplay(run, \"\")"));
	assert!(!response.contains("parts.splice(2, 0, `rate limit ${rateLimit}`);"));
	assert!(!response.contains("`rate ${protocolActivityRateLimitDisplay(run)}`"));
	assert!(response.contains("facts.push([\"focus\", detailLabel(focus)]);"));
	assert!(response.contains("return \"approval/user input\";"));
	assert!(response.contains("return \"protocol idleness\";"));
	assert!(response.contains("field(\"Protocol activity\", protocolActivityDebugSummary(run))"));
	assert!(!response.contains("field(\"Rate limit\", protocolActivityRateLimitDisplay(run))"));
}

#[test]
fn operator_dashboard_run_activity_consumes_server_presentation() {
	let response = dashboard::dashboard_response();

	assert!(response.contains("function presentationCurrentLaneCards(presentation)"));
	assert!(response.contains("function snapshotCurrentLaneCards(snapshot)"));
	assert!(response.contains("return presentationCurrentLaneCards(snapshot?.presentation);"));
	assert!(response.contains("function currentLaneRunsFromCards(cards)"));
	assert!(response.contains("function currentLaneCardToneClass(card)"));
	assert!(response.contains("if (card?.is_waiting === true || card?.tone === \"waiting\")"));
	assert!(response.contains("if (card?.counts_as_running === true || runCountsAsRunning(run))"));
	assert!(response.contains("function dashboardRunActivityIsStale(payload)"));
	assert!(response.contains("emittedAt < snapshotPublishedAt"));
	assert!(response.contains("if (dashboardRunActivityIsStale(payload))"));
	assert!(response.contains("let dashboardLivePresentation = null;"));
	assert!(response.contains("let dashboardLiveRunActivitySeen = false;"));
	assert!(!response.contains("let dashboardLiveAccounts = null;"));
	assert!(response.contains("let dashboardLiveAccountControl = null;"));
	assert!(response.contains(
		"function dashboardLiveRunActivityHasOverlay({ includeCompletedEmpty = false } = {})"
	));
	assert!(response.contains("function clearDashboardLiveRunActivityOverlayIfCompleteEmpty()"));
	assert!(response.contains("return includeCompletedEmpty;"));
	assert!(response.contains("function clearDashboardLiveRunActivityOverlay()"));
	assert!(response.contains("function snapshotWithLiveRunActivity(snapshot, options = {})"));
	assert!(response.contains("if (!dashboardLiveRunActivityHasOverlay(options))"));
	assert!(!response.contains("field(\"Author\","));
	assert!(!response.contains("\"author\",\n"));
	assert!(response.contains("payload.accountControl"));
	assert!(!response.contains("activityPayload.accounts"));
	assert!(!response.contains("payload.accounts"));
	assert!(!response.contains("dashboardLiveAccounts"));
	assert!(response.contains("dashboardLiveAccountControl ="));
	assert!(response.contains("dashboardLivePresentation = {"));
	assert!(
		response
			.contains("current_lane_cards: presentationCurrentLaneCards(payload.presentation),")
	);
	assert!(response.contains("dashboardLiveRunActivitySeen = true;"));
	assert!(response.contains("clearDashboardLiveRunActivityOverlay();"));
	assert!(response.contains("snapshot: payload.snapshot,"));
	assert!(response.contains(
		"snapshot: snapshotWithLiveRunActivity(lastDashboardRender.snapshot, {\n\t\t\t\t\t\tincludeCompletedEmpty: true,\n\t\t\t\t\t}),"
	));
	assert!(response.contains("clearDashboardLiveRunActivityOverlayIfCompleteEmpty();"));
	assert!(response.contains("account_control:"));
	assert!(!response.contains("accounts: dashboardLiveAccounts"));
	assert!(response.contains("current_lanes: liveRuns,"));
	assert!(response.contains("presentation: dashboardLivePresentation,"));
	assert!(!response.contains("snapshot?.current_lanes ?? []"));
	assert!(!response.contains("issueDisplayKey(run),\n\t\t\t\t\t\t\trun_id: run.run_id"));
	assert!(!response.contains("function currentLaneSummary"));
	assert!(!response.contains("function runIssueTitle"));
	assert!(!response.contains("card.title || runIssueTitle"));
	assert!(!response.contains("currentLaneSummary(run) || card.detail"));
	assert!(!response.contains("function currentLaneCardToneClass(card, run)"));
	assert!(!response.contains("function mergeDashboardRunRecord"));
	assert!(!response.contains("function mergeDashboardCurrentLanes"));
	assert!(!response.contains("function mergeDashboardRunActivity"));
}

#[test]
fn operator_dashboard_uses_websocket_without_http_state_fallback() {
	let response = dashboard::dashboard_response();

	assert!(response.contains("connectDashboardSocket();"));
	assert!(response.contains("function startDashboardStream()"));
	assert!(response.contains("startDashboardStream();"));
	assert!(response.contains("document.addEventListener(\"visibilitychange\", () => {"));
	assert!(response.contains("if (document.hidden) {\n\t\t\t\t\treturn;\n\t\t\t\t}"));
	assert!(
		response.contains("if (!dashboardSocketIsOpen()) {\n\t\t\t\t\tconnectDashboardSocket();")
	);
	assert!(response.contains("function renderDashboardLocalClockTick()"));
	assert!(response.contains("const ACCOUNT_API_REFRESH_INTERVAL_MS = 15_000;"));
	assert!(response.contains("now - accountApiRefreshedAt < ACCOUNT_API_REFRESH_INTERVAL_MS"));
	assert!(response.contains("const response = await fetch(\"/api/accounts?refresh=1\""));
	assert!(response.contains("refreshAccountApiSnapshot();"));
	assert!(
		response.contains("renderDashboardState(lastDashboardRender, { refreshAccounts: false });")
	);
	assert!(response.contains("const shouldRefreshAccounts = options.refreshAccounts !== false;"));
	assert!(!response.contains("function scheduleDashboardHttpFallback"));
	assert!(!response.contains("clearDashboardHttpFallback();"));
	assert!(!response.contains("requestJson("));
	assert!(!response.contains("requestText("));
	assert!(!response.contains("\"/state\""));
	assert!(!response.contains("\"/readyz\""));
	assert!(!response.contains("window.setInterval(refreshDashboard"));
	assert!(!response.contains("function refreshDashboard"));
	assert!(!response.contains("const REFRESH_INTERVAL_MS"));
}
