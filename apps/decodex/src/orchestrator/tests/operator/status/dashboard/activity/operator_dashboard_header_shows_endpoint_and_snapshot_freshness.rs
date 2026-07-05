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
