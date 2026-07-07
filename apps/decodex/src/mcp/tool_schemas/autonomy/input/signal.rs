use serde_json::{self, Value};

use crate::mcp::tool_schemas;

pub(in crate::mcp) fn autonomy_submit_signal_tool_input_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"mode": {
				"type": "string",
				"enum": ["dry_run", "apply"],
				"description": "dry_run validates the signal; apply persists proposal-only signal evidence."
			},
			"projectId": {
				"type": "string",
				"description": "Optional Decodex service id when the MCP context is not project-scoped."
			},
			"kind": {
				"type": "string",
				"enum": [
					"runtime_health",
					"validation_regression",
					"review_feedback_cluster",
					"user_feedback_cluster",
					"spec_drift",
					"protocol_drift",
					"metric_regression",
					"execution_friction",
					"docs_plugin_drift"
				]
			},
			"signal": {
				"type": "object",
				"additionalProperties": true,
				"description": "Signal input without derived id/fingerprint; Decodex derives stable identity."
			},
			"authority": tool_schemas::planning_authority_input_schema()
		},
		"required": ["kind", "signal"]
	})
}
