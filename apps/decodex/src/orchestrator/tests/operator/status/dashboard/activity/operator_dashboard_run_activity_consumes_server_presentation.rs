use crate::orchestrator::tests::operator::status::dashboard;

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
