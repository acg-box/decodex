use crate::orchestrator::tests::operator::status::dashboard;

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
