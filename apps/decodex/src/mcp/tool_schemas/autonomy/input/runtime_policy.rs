use serde_json::{self, Value};

pub(in crate::mcp) fn autonomy_accept_runtime_policy_tool_input_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"mode": {
				"type": "string",
				"enum": ["dry_run"],
				"description": "Validates and previews the exact registered binding without authority mutation. Acceptance is available only through the interactive Decodex operator CLI ceremony."
			},
			"projectId": { "type": "string" },
			"publicNonGoals": {
				"type": "array",
				"minItems": 1,
				"items": { "type": "string" },
				"description": "Explicitly accepted public-safe non-goal projection used in generated Decision Contracts."
			},
			"authority": {
				"type": "object",
				"additionalProperties": false,
				"properties": {
					"acceptedBy": { "type": "string" },
					"acceptedByKind": { "type": "string", "enum": ["user"] },
					"acceptedAt": { "type": "string" },
					"acceptanceSource": { "type": "string" }
				},
				"required": ["acceptedBy", "acceptedByKind", "acceptedAt", "acceptanceSource"]
			}
		},
		"required": []
	})
}

pub(in crate::mcp) fn autonomy_apply_runtime_policy_tool_input_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"mode": {
				"type": "string",
				"enum": ["dry_run", "apply"],
				"description": "dry_run evaluates trusted state without mutation; apply records Decodex's internal challenge and promotes a Decision Contract only."
			},
			"projectId": { "type": "string" },
			"proposalId": { "type": "string" }
		},
		"required": ["proposalId"]
	})
}
