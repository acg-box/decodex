use serde_json::{self, Value};

use crate::mcp::tool_schemas;

pub(in crate::mcp) fn observe_tool_output_schema() -> Value {
	tool_schemas::tool_output_schema(serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"schema": {
				"type": "string",
				"enum": ["decodex.mcp.observe_result/1"]
			},
			"status": {
				"type": "string",
				"enum": ["ok"]
			},
			"capability_profile": {
				"type": "string",
				"enum": ["observe"]
			},
			"observability": {
				"type": "object",
				"additionalProperties": true
			}
		},
		"required": ["schema", "status", "capability_profile", "observability"]
	}))
}

pub(in crate::mcp) fn plan_tool_output_schema() -> Value {
	tool_schemas::tool_output_schema(serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"schema": {
				"type": "string",
				"enum": ["decodex.mcp.plan_result/2"]
			},
			"status": {
				"type": "string",
				"enum": ["ok"]
			},
			"intent": {
				"type": "string",
				"enum": ["validation_ready", "handoff", "lane_control", "intake_goal"]
			},
			"prompt": {
				"type": "string"
			},
			"next_action": {
				"type": "string"
			},
			"issue": {
				"type": ["string", "null"]
			},
			"contract_id": {
				"type": ["string", "null"]
			}
		},
		"required": ["schema", "status", "intent", "prompt", "next_action"]
	}))
}
