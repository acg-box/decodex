use serde_json::{self, Value};

use crate::mcp::tool_schemas;

pub(in crate::mcp) fn lane_control_tool_output_schema() -> Value {
	tool_schemas::tool_output_schema(serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"schema": {
				"type": "string",
				"enum": ["decodex.mcp.lane_control_result/1"]
			},
			"status": {
				"type": "string",
				"enum": ["ok", "queued", "refused"]
			},
			"reason": {
				"type": "string"
			},
			"message": {
				"type": "string"
			},
			"capability_profile": {
				"type": "string",
				"enum": ["operate"]
			},
			"action": {
				"type": "string",
				"enum": ["inspect", "interrupt", "steer", "manual_attention", "retained_resume"]
			},
			"project_id": {
				"type": ["string", "null"]
			},
			"issue": {
				"type": ["string", "null"]
			},
			"run_id": {
				"type": ["string", "null"]
			},
			"preconditions": {
				"type": "object",
				"additionalProperties": false,
				"properties": {
					"project_id_present": { "type": "boolean" },
					"issue_present": { "type": "boolean" },
					"run_id_present": { "type": "boolean" },
					"expected_turn_id_present": { "type": "boolean" },
					"message_present": { "type": "boolean" },
					"force_requested": { "type": "boolean" },
					"authority_reason_present": { "type": "boolean" },
					"authority_source_present": { "type": "boolean" },
					"authority_inspected_run_id_present": { "type": "boolean" },
					"authority_expected_turn_id_present": { "type": "boolean" },
					"authority_allow_hard_fallback": { "type": "boolean" }
				},
				"required": [
					"project_id_present",
					"issue_present",
					"run_id_present",
					"expected_turn_id_present",
					"message_present",
					"force_requested",
					"authority_reason_present",
					"authority_source_present",
					"authority_inspected_run_id_present",
					"authority_expected_turn_id_present",
					"authority_allow_hard_fallback"
				]
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
			"preconditions",
			"result"
		]
	}))
}
