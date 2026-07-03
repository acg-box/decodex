use crate::orchestrator::tests::operator::status::http::{
	OPERATOR_DASHBOARD_ALIAS_ENDPOINT_PATH, OPERATOR_DASHBOARD_ENDPOINT_PATH, orchestrator,
};
#[test]
fn operator_state_endpoint_serves_dashboard_html_from_root_and_dashboard_route() {
	for path in [OPERATOR_DASHBOARD_ENDPOINT_PATH, OPERATOR_DASHBOARD_ALIAS_ENDPOINT_PATH] {
		let response = String::from_utf8(
			orchestrator::build_operator_state_http_response(
				format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
					.as_bytes(),
			)
			.expect("dashboard response should build"),
		)
		.expect("dashboard response should be utf-8");

		assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
		assert!(response.contains("Content-Type: text/html; charset=utf-8"));
		assert!(response.contains("<title>Decodex</title>"));
		assert!(response.contains("<h1 id=\"project-title\">Decodex</h1>"));
		assert!(response.contains("Delivery flow"));
		assert!(response.contains("flow-queue"));
		assert!(response.contains("<span>Intake</span>"));
		assert!(response.contains("<span>Landing</span>"));
		assert!(response.contains("section-marker section-marker-projects"));
		assert!(!response.contains("<h2 id=\"projects-title\">Projects</h2>"));
		assert!(!response.contains("data-fold-key=\"panel:projects\""));
		assert!(response.contains("id=\"project-filter-toggle\""));
		assert!(response.contains("class=\"project-table\" role=\"table\""));
		assert!(!response.contains("<h2>All</h2>"));
		assert!(response.contains("projectRegistrationCommand"));
		assert!(response.contains("decodex project add ~/.codex/decodex/projects/<service-id>"));
		assert!(response.contains("Register projects explicitly"));
		assert!(response.contains("does not scan history or repos"));
		assert!(response.contains("data-detail-key"));
		assert!(response.contains("notice-dock"));
		assert!(response.contains("Notices"));
		assert!(response.contains("notice-panel"));
		assert!(response.contains("Snapshot stream"));
		assert!(response.contains("Snapshot warning"));
		assert!(response.contains("Tracker sync paused"));
		assert!(response.contains("connector_backoffs"));
		assert!(response.contains("Sync backoff"));
		assert!(response.contains("project_id"));
		assert!(response.contains("retry_after_seconds"));
		assert!(response.contains("reset_at"));
		assert!(response.contains("sync_phase"));
		assert!(!response.contains("error-banner"));
		assert!(!response.contains("metric-active"));
		assert!(!response.contains("Queued issue -> reviewed change -> landed branch"));
		assert!(response.contains("Current Lanes"));
		assert!(response.contains("Intake Queue"));
		assert!(response.contains("Review &amp; Landing"));
		assert!(response.contains("Run History"));
		assert!(response.contains("historyLedgerOutcome"));
		assert!(response.contains("Run history unavailable"));
		assert!(response.contains("renderHistoryLedgerFacts"));
		assert!(response.contains("Recovery Worktrees"));
		assert!(response.contains("Lane activity"));
		assert!(response.contains("agent idle"));
		assert!(response.contains("Child agent"));
		assert!(response.contains("<span>Activity</span>"));
		assert!(!response.contains("<span>Agent Now</span>"));
		assert!(response.contains("current window"));
		assert!(response.contains("peak window"));
		assert!(!response.contains("same as current"));
		assert!(response.contains("Context lifecycle metrics"));
		assert!(response.contains("rows.push({ label: \"Context\", segments: contextSegments });"));
		assert!(response.contains("renderChildLifecyclePhaseTable(lifecycle.phases || [])"));
		assert!(response.contains("facts.push([\"tokens\", tokenSummary]);"));
		assert!(!response.contains("\"cumulative input\","));
		assert!(response.contains("Current context window from the latest child-agent event."));
		assert!(response.contains("child_agent_activity"));
		assert!(response.contains("renderChildAgentBreakdown"));
		assert!(response.contains("Debug Details"));
		assert!(response.contains("already running"));
		assert!(!response.contains("running laness"));
		assert!(!response.contains("active-echo"));
		assert!(response.contains("fold-panel"));
		assert!(response.contains(".fold-indicator::before"));
		assert!(response.contains("content: \"+\";"));
		assert!(response.contains("content: \"-\";"));
		assert!(!response.contains(".fold-indicator::after"));
		assert!(response.contains("data-fold-key=\"panel:worktrees\""));
		assert!(response.contains("data-fold-key=\"panel:recent\""));
		assert!(response.contains("cursor: pointer;"));
		assert!(response.contains("animateDetail(details, !details.open)"));
		assert!(response.contains("width: min(380px, calc(100vw - 36px));"));
		assert!(response.contains(".notice-item p"));
		assert!(response.contains("font-size: var(--type-body);"));
		assert!(!response.contains(".fold-panel.is-empty .fold-indicator"));
		assert!(!response.contains("details.classList.contains(\"is-empty\")"));
		assert!(!response.contains("Operator views"));
		assert!(!response.contains("Command Brief"));
		assert!(!response.contains("Intake Pressure"));
		assert!(!response.contains("Landing Readiness"));

		assert_dashboard_html_control_surface(response.as_str());

		assert!(!response.contains("Last updated: none"));
		assert!(!response.contains("Auto-refresh"));
		assert!(!response.contains("<h2>Project Scope</h2>"));
		assert!(!response.contains("Projects appear on the first state update"));
		assert!(!response.contains("Diagnostics"));
		assert!(!response.contains("State JSON"));
		assert!(!response.contains("Ready probe"));
		assert!(!response.contains("Live probe"));
		assert!(!response.contains("/livez"));
	}
}

fn assert_dashboard_html_control_surface(response: &str) {
	for required in [
		"/dashboard/control",
		"WebSocket",
		"applyDashboardRunActivity",
		"sendDashboardControl",
		"controlAck",
	] {
		assert!(
			response.contains(required),
			"missing required dashboard control marker `{required}`"
		);
	}
	for forbidden in [
		"/state",
		"/readyz",
		"data-dashboard-control=\"interruptRun\"",
		"aria-label=\"Stop this active Decodex work\"",
		"runInterruptControlEnabled",
		"renderRunStopControl",
		"action === \"interruptRun\"",
		"case \"interruptRun\"",
		"data-dashboard-control=\"focusProject\"",
		"data-dashboard-control=\"focusRun\"",
		"data-dashboard-control=\"pauseProject\"",
		"data-dashboard-control=\"resumeProject\"",
		"data-dashboard-control=\"retryRun\"",
		">Retry now</button>",
		">Retry</button>",
		"run.wait_reason),",
	] {
		assert!(!response.contains(forbidden), "unexpected dashboard control marker `{forbidden}`");
	}
}

#[test]
fn operator_dashboard_uses_decodex_brand_icons() {
	let response = String::from_utf8(
		orchestrator::build_operator_state_http_response(
			format!(
				"GET {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
				orchestrator::OPERATOR_DASHBOARD_ENDPOINT_PATH
			)
			.as_bytes(),
		)
		.expect("dashboard response should build"),
	)
	.expect("dashboard response should be utf-8");

	assert!(response.contains(r#"<link rel="icon" type="image/png" href="/assets/icon.png" />"#));
	assert!(response.contains(r#"<link rel="icon" href="/assets/logo.ico" />"#));
	assert!(response.contains(
		r#"<link rel="apple-touch-icon" sizes="180x180" href="/assets/logo-touch.png" />"#
	));
	assert!(!response.contains("data:image/svg+xml"));
	assert!(!response.contains("M18 57V23"));
}

#[test]
fn operator_state_endpoint_serves_decodex_brand_assets() {
	for (path, content_type, signature) in [
		("/assets/icon.png", "image/png", b"\x89PNG\r\n\x1a\n".as_slice()),
		("/assets/logo-touch.png", "image/png", b"\x89PNG\r\n\x1a\n".as_slice()),
		("/assets/logo.ico", "image/x-icon", b"\0\0\x01\0".as_slice()),
	] {
		let response = orchestrator::build_operator_state_http_response(
			format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
				.as_bytes(),
		)
		.expect("asset response should build");
		let header_end = response
			.windows(4)
			.position(|window| window == b"\r\n\r\n")
			.expect("response should contain headers");
		let headers =
			String::from_utf8(response[..header_end].to_vec()).expect("headers should be utf-8");
		let body = &response[(header_end + 4)..];

		assert!(headers.starts_with("HTTP/1.1 200 OK\r\n"));
		assert!(headers.contains(&format!("Content-Type: {content_type}")));
		assert!(body.starts_with(signature));
	}
}

#[test]
fn operator_state_endpoint_rejects_dashboard_websocket_without_upgrade() {
	let response = String::from_utf8(
		orchestrator::build_operator_state_http_response(
			format!(
				"GET {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
				orchestrator::OPERATOR_DASHBOARD_WS_ENDPOINT_PATH
			)
			.as_bytes(),
		)
		.expect("dashboard websocket response should build"),
	)
	.expect("dashboard websocket response should be utf-8");

	assert!(response.starts_with("HTTP/1.1 426 Upgrade Required\r\n"));
	assert!(response.contains("Upgrade: websocket"));
	assert!(response.ends_with("websocket upgrade required"));
}
