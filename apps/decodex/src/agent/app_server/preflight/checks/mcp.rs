use crate::agent::app_server::preflight::{
	AppServerCapabilityPreflightReport, AppServerOutputTimeout, BTreeMap,
	MCP_PREFLIGHT_REQUEST_TIMEOUT, McpServerStatusSummary, PREFLIGHT_CHECK_MCP, Report,
};

pub(crate) fn record_mcp_preflight(
	report: &mut AppServerCapabilityPreflightReport,
	servers: &[McpServerStatusSummary],
) {
	let not_logged_in = servers
		.iter()
		.filter(|server| server.auth_status == "notLoggedIn")
		.map(|server| server.name.clone())
		.collect::<Vec<_>>();
	let tool_count: usize = servers.iter().map(|server| server.tools.len()).sum();
	let mut details = BTreeMap::new();

	details.insert(String::from("server_count"), servers.len().to_string());
	details.insert(String::from("tool_count"), tool_count.to_string());

	if !not_logged_in.is_empty() {
		details.insert(String::from("not_logged_in_servers"), not_logged_in.join(", "));
	}
	if !not_logged_in.is_empty() {
		report.push_blocked(
			PREFLIGHT_CHECK_MCP,
			"mcpServerStatus/list returned MCP servers that are not logged in.",
			details,
		);
	} else {
		report.push_ok(
			PREFLIGHT_CHECK_MCP,
			"mcpServerStatus/list returned MCP server state.",
			details,
		);
	}
}

pub(crate) fn mcp_preflight_can_degrade(error: &Report) -> bool {
	preflight_error_timed_out(error)
}

pub(crate) fn record_mcp_preflight_degraded(
	report: &mut AppServerCapabilityPreflightReport,
	error: &Report,
) {
	let mut details = BTreeMap::new();

	details.insert(String::from("method"), String::from("mcpServerStatus/list"));
	details.insert(String::from("degraded_reason"), String::from("timeout"));
	details.insert(String::from("error"), error.to_string());
	details.insert(
		String::from("timeout_seconds"),
		MCP_PREFLIGHT_REQUEST_TIMEOUT.as_secs().to_string(),
	);
	report.push_ok(
		PREFLIGHT_CHECK_MCP,
		"mcpServerStatus/list timed out during optional MCP inventory; continuing after core app-server capability checks passed.",
		details,
	);
}

pub(crate) fn preflight_error_timed_out(error: &Report) -> bool {
	error.downcast_ref::<AppServerOutputTimeout>().is_some()
}
