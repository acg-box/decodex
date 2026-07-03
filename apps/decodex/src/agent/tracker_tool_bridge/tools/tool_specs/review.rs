use crate::agent::tracker_tool_bridge::{
	DynamicToolSpec, ISSUE_REVIEW_CHECKPOINT_TOOL_NAME, ISSUE_REVIEW_HANDOFF_TOOL_NAME,
	ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME, ISSUE_TERMINAL_FINALIZE_TOOL_NAME, TrackerToolBridge,
	tools::review_checkpoint::{
		non_empty_string_array_schema, review_checkpoint_checks_schema,
		review_checkpoint_contract_schema, review_checkpoint_finding_routes_schema,
		review_checkpoint_findings_array_schema, review_checkpoint_reviewer_schema,
		review_checkpoint_status_schema, review_cost_control_schema,
	},
};

impl<'a> TrackerToolBridge<'a> {
	pub(super) fn review_handoff_tool_specs(&self) -> [DynamicToolSpec; 2] {
		[
			DynamicToolSpec::new(
				ISSUE_REVIEW_HANDOFF_TOOL_NAME,
				"Record a PR-backed review handoff for the currently leased issue after the branch is pushed and a non-draft PR is ready for review.",
				serde_json::json!({
					"type": "object",
					"properties": {
						"issue_id": { "type": "string" },
						"issue_identifier": { "type": "string" },
						"pr_url": { "type": "string" },
						"summary": { "type": "string" }
					},
					"required": ["pr_url", "summary"],
					"additionalProperties": false
				}),
			),
			DynamicToolSpec::new(
				ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
				"Finalize the current run's terminal tracker path after either PR-backed review handoff or the manual-attention exit has been fully recorded.",
				serde_json::json!({
					"type": "object",
					"properties": {
						"issue_id": { "type": "string" },
						"issue_identifier": { "type": "string" },
						"path": {
							"type": "string",
							"enum": ["review_handoff", "manual_attention"]
						}
					},
					"required": ["path"],
					"additionalProperties": false
				}),
			),
		]
	}

	pub(super) fn review_checkpoint_tool_specs(&self) -> [DynamicToolSpec; 1] {
		[DynamicToolSpec::new(
			ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
			"Record the independent fresh-context read-only bounded-review result for the leased issue so Decodex can decide whether the lane may continue, repair accepted findings, or stop for human intervention. `head_sha` must resolve to the current lane HEAD.",
			serde_json::json!({
				"type": "object",
				"properties": {
					"issue_id": { "type": "string" },
					"issue_identifier": { "type": "string" },
					"reviewer": review_checkpoint_reviewer_schema(),
					"status": review_checkpoint_status_schema(),
					"head_sha": { "type": "string" },
					"review_contract": review_checkpoint_contract_schema(),
					"review_cost_control": review_cost_control_schema(),
					"checks": review_checkpoint_checks_schema(),
					"evidence": non_empty_string_array_schema(),
					"accepted_findings": review_checkpoint_findings_array_schema(false),
					"rejected_findings": review_checkpoint_findings_array_schema(true),
					"finding_routes": review_checkpoint_finding_routes_schema()
				},
				"required": ["reviewer", "status", "head_sha", "review_contract", "checks", "evidence"],
				"additionalProperties": false
			}),
		)]
	}

	pub(super) fn review_repair_tool_specs(&self) -> [DynamicToolSpec; 2] {
		[
			DynamicToolSpec::new(
				ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME,
				"Record that the retained in-review lane repaired the current PR head, pushed it, and requested fresh review on the same PR lineage.",
				serde_json::json!({
					"type": "object",
					"properties": {
						"issue_id": { "type": "string" },
						"issue_identifier": { "type": "string" },
						"pr_url": { "type": "string" },
						"summary": { "type": "string" }
					},
					"required": ["pr_url", "summary"],
					"additionalProperties": false
				}),
			),
			DynamicToolSpec::new(
				ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
				"Finalize the current run's terminal tracker path after either retained review repair or the manual-attention exit has been fully recorded.",
				serde_json::json!({
					"type": "object",
					"properties": {
						"issue_id": { "type": "string" },
						"issue_identifier": { "type": "string" },
						"path": {
							"type": "string",
							"enum": ["review_repair", "manual_attention"]
						}
					},
					"required": ["path"],
					"additionalProperties": false
				}),
			),
		]
	}
}
