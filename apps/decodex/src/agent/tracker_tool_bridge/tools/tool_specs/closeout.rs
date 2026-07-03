use crate::agent::tracker_tool_bridge::{
	DynamicToolSpec, ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME, ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
	TrackerToolBridge,
};

impl<'a> TrackerToolBridge<'a> {
	pub(super) fn closeout_tool_specs(&self) -> [DynamicToolSpec; 2] {
		[
			DynamicToolSpec::new(
				ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME,
				"Record that the retained post-review lane finished merge plus closeout for the same owned PR lineage.",
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
				"Finalize the current run's terminal tracker path after either post-review closeout or the manual-attention exit has been fully recorded.",
				serde_json::json!({
					"type": "object",
					"properties": {
						"issue_id": { "type": "string" },
						"issue_identifier": { "type": "string" },
						"path": {
							"type": "string",
							"enum": ["closeout", "manual_attention"]
						}
					},
					"required": ["path"],
					"additionalProperties": false
				}),
			),
		]
	}
}
