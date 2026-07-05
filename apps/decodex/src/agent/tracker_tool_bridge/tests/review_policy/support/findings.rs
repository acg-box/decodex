use serde_json::Value;

pub(in crate::agent::tracker_tool_bridge::tests) fn accepted_review_findings_json() -> Value {
	accepted_review_findings_with_summary_json(
		"Accepted reviewer finding",
		"Repair the accepted issue before requesting another review checkpoint.",
		1,
	)
}

pub(in crate::agent::tracker_tool_bridge::tests) fn accepted_review_findings_with_summary_json(
	summary: &str,
	guidance: &str,
	line: u64,
) -> Value {
	serde_json::json!([{
		"severity": "medium",
		"summary": summary,
		"evidence": ["The reviewer evidence points at the current lane head."],
		"file": "apps/decodex/src/agent/tracker_tool_bridge/tools.rs",
		"line": line,
		"guidance": guidance
	}])
}

pub(in crate::agent::tracker_tool_bridge::tests) fn accepted_review_findings_for_status_json(
	status: &str,
) -> Value {
	if status == "findings" { accepted_review_findings_json() } else { serde_json::json!([]) }
}

pub(in crate::agent::tracker_tool_bridge::tests) fn route_only_review_route_json(
	route: &str,
) -> Value {
	serde_json::json!([{
		"route": route,
		"severity": "medium",
		"risk_tier": "medium",
		"summary": "Review signal is routed outside current repair.",
		"evidence": ["The reviewer signal was checked against the current lane head."],
		"resolver": "architecture",
		"next_action": "Record the routed review signal without mutating the current repair."
	}])
}
