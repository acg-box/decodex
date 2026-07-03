use crate::{orchestrator, orchestrator::tests::operator::status::dashboard};

#[test]
fn operator_app_snapshot_endpoint_returns_json() {
	let response = String::from_utf8(
		orchestrator::build_operator_state_http_response(
			format!(
				"GET {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
				orchestrator::OPERATOR_APP_SNAPSHOT_ENDPOINT_PATH
			)
			.as_bytes(),
		)
		.expect("app snapshot response should build"),
	)
	.expect("app snapshot response should be utf-8");

	assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
	assert!(response.contains("Content-Type: application/json\r\n"));
	assert!(response.ends_with("\r\n\r\n{}"));
}

#[test]
fn operator_dashboard_surfaces_loop_status_fields() {
	let response = dashboard::dashboard_response();

	assert!(response.contains("function loopStatusFacts(loopStatus)"));
	assert!(response.contains("function loopStatusInline(loopStatus)"));
	assert!(response.contains("loopStatusInline(run.loop_status)"));
	assert!(response.contains("loopStatusFacts(run.loop_status)"));
	assert!(response.contains("loopStatusFacts(lane.loop_status)"));
	assert!(response.contains("loopStatusFacts(attention.loop_status)"));
	assert!(response.contains("function autonomyReadbackHasFreshSourceRefs(loopStatus)"));
	assert!(response.contains(
		"return sourceRefs.length > 0 && String(signal?.freshness || \"\") === \"fresh\";"
	));
	assert!(response.contains("function autonomyReadbackSummary(loopStatus)"));
	assert!(
		response.contains("field(\"Autonomy readback\", autonomyReadbackSummary(run.loop_status))")
	);
	assert!(!response.contains("facts.push([\"Autonomy\", displayToken(loopStatus.autonomy)]);"));
}

#[test]
fn operator_dashboard_surfaces_program_intake_panel() {
	let response = dashboard::dashboard_response();

	assert!(response.contains("id=\"programs-panel\""));
	assert!(response.contains("id=\"programs-meta\""));
	assert!(response.contains("id=\"execution-programs\""));
	assert!(response.contains("<h2 id=\"programs-title\">Program Intake</h2>"));
	assert!(response.contains("programs: document.getElementById(\"programs-panel\")"));
	assert!(
		response.contains("executionPrograms: document.getElementById(\"execution-programs\")")
	);
	assert!(response.contains("programsMeta: document.getElementById(\"programs-meta\")"));
	assert!(response.contains("function renderExecutionPrograms(snapshot, derived)"));
	assert!(response.contains("function renderProgramNodeReadbacks(program)"));
	assert!(response.contains("program.node_readbacks ?? []"));
	assert!(response.contains("program.dispatchable_count"));
	assert!(
		response
			.contains("field(\"Program stage\", displayToken(node.program_stage || \"unknown\"))")
	);
	assert!(response.contains("node.dispatch_action"));
	assert!(response.contains("renderExecutionPrograms(snapshot, derived);"));
	assert!(response.contains("primary: [\"accountPool\", \"projects\", \"currentLanes\", \"programs\", \"queue\", \"review\", \"worktrees\", \"recent\"]"));
	assert!(response.contains(
		"{ marker: \"execution\", panels: [\"currentLanes\", \"programs\", \"queue\"] }"
	));
	assert!(!response.contains("data-program-edit"));
	assert!(!response.contains("data-program-mutate"));
}

#[test]
fn operator_dashboard_background_wash_stays_viewport_fixed() {
	let response = dashboard::dashboard_response();

	assert!(response.contains("background-attachment: fixed, fixed, fixed, scroll;"));
	assert!(response.contains("background-size: 100vw 100vh, 100vw 100vh, 100vw 100vh, auto;"));
}
