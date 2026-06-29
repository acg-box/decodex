use serde_json::{self, Value};

use crate::mcp::tool_schemas;

pub(in crate::mcp) fn project_control_tool_output_schema() -> Value {
	tool_schemas::tool_output_schema(serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"schema": {
				"type": "string",
				"enum": ["decodex.mcp.project_control_result/1"]
			},
			"status": {
				"type": "string",
				"enum": ["ok", "refused"]
			},
			"reason": {
				"type": "string"
			},
			"message": {
				"type": "string"
			},
			"capability_profile": {
				"type": "string",
				"enum": ["admin"]
			},
			"action": {
				"type": "string",
				"enum": ["status", "pause", "resume", "scan"]
			},
			"project_id": {
				"type": ["string", "null"]
			},
			"future_dispatch_only": {
				"type": "boolean"
			},
			"result": {
				"type": "object",
				"additionalProperties": true
			}
		},
		"required": [
			"schema",
			"status",
			"reason",
			"message",
			"capability_profile",
			"action",
			"future_dispatch_only",
			"result"
		]
	}))
}
