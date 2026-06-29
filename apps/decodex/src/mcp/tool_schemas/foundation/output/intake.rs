use serde_json::{self, Value};

use crate::mcp::tool_schemas;

pub(in crate::mcp) fn intake_goal_tool_output_schema() -> Value {
	tool_schemas::tool_output_schema(serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"schema": { "type": "string", "enum": ["decodex.mcp.intake_goal_result/1"] },
			"status": { "type": "string", "enum": ["ok"] },
			"mode": { "type": "string", "enum": ["dry_run", "apply"] },
			"service_id": { "type": "string" },
			"contract_id": { "type": "string" },
			"dry_run": { "type": "boolean" },
			"applied": { "type": "boolean" },
			"persisted": { "type": "boolean" },
			"issue_count": { "type": "integer", "minimum": 0 },
			"issues": {
				"type": "array",
				"items": {
					"type": "object",
					"additionalProperties": false,
					"properties": {
						"title": { "type": "string" },
						"objective": { "type": "string" },
						"issue_identifier": { "type": ["string", "null"] },
						"action": { "type": "string" },
						"dependencies": { "type": "array", "items": { "type": "string" } },
						"conflict_domains": { "type": "array", "items": { "type": "string" } },
						"acceptance": { "type": "array", "items": { "type": "string" } },
						"validation": { "type": "array", "items": { "type": "string" } },
						"reasons": { "type": "array", "items": { "type": "string" } }
					},
					"required": [
						"title",
						"objective",
						"issue_identifier",
						"action",
						"dependencies",
						"conflict_domains",
						"acceptance",
						"validation",
						"reasons"
					]
				}
			},
			"next_action": { "type": "string" }
		},
		"required": [
			"schema",
			"status",
			"mode",
			"service_id",
			"contract_id",
			"dry_run",
			"applied",
			"persisted",
			"issue_count",
			"issues",
			"next_action"
		]
	}))
}
