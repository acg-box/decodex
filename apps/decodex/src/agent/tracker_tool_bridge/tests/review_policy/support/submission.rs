use serde_json::Value;

use crate::agent::tracker_tool_bridge::{
	DynamicToolCallResponse, DynamicToolHandler, ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
	TrackerToolBridge,
	tests::{self, review_policy},
};

pub(in crate::agent::tracker_tool_bridge::tests) fn submit_findings_review_checkpoint(
	bridge: &TrackerToolBridge<'_>,
	evidence: &str,
) -> DynamicToolCallResponse {
	submit_findings_review_checkpoint_with_findings(
		bridge,
		evidence,
		review_policy::accepted_review_findings_json(),
	)
}

pub(in crate::agent::tracker_tool_bridge::tests) fn submit_findings_review_checkpoint_with_findings(
	bridge: &TrackerToolBridge<'_>,
	evidence: &str,
	accepted_findings: Value,
) -> DynamicToolCallResponse {
	DynamicToolHandler::handle_call(
		bridge,
		ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"reviewer": "independent_fresh_context",
			"status": "findings",
			"head_sha": tests::sample_local_repo().head_oid,
			"review_contract": crate::agent::tracker_tool_bridge::tests::review_policy::handoff_review_contract_json(),
			"checks": crate::agent::tracker_tool_bridge::tests::review_policy::review_checks_json(),
			"evidence": [evidence],
			"accepted_findings": accepted_findings
		}),
	)
}
