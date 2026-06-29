use serde_json::{self, Value};

use super::tool_output_schema;

pub(in crate::mcp) fn lane_control_tool_input_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"action": {
				"type": "string",
				"enum": ["inspect", "interrupt", "steer", "manual_attention", "retained_resume"]
			},
			"projectId": {
				"type": "string",
				"description": "Optional project id precondition. When supplied, it must match the MCP gateway project context."
			},
			"issue": {
				"type": "string",
				"description": "Issue identifier or tracker issue id."
			},
			"runId": {
				"type": "string",
				"description": "Current run id observed through inspect."
			},
			"expectedTurnId": {
				"type": "string",
				"description": "Current turn id required for steer."
			},
			"message": {
				"type": "string",
				"description": "Operator-supplied steer message."
			},
			"force": {
				"type": "boolean",
				"description": "Hard interrupt fallback is not exposed through MCP and is refused when true."
			},
			"authority": {
				"type": "object",
				"additionalProperties": false,
				"properties": {
					"reason": {
						"type": "string",
						"description": "Explicit operator reason for a mutating lane-control request."
					},
					"source": {
						"type": "string",
						"description": "Remote client or operator source identifier."
					},
					"inspectedRunId": {
						"type": "string",
						"description": "Run id observed through a prior inspect call."
					},
					"expectedTurnId": {
						"type": "string",
						"description": "Turn id observed through inspect and required for steer."
					},
					"allowHardFallback": {
						"type": "boolean",
						"description": "Explicit acknowledgement required with force=true before hard interrupt fallback can run."
					}
				}
			}
		},
		"required": ["action"]
	})
}

pub(in crate::mcp) fn project_control_tool_input_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"action": {
				"type": "string",
				"enum": ["status", "pause", "resume", "scan"],
				"description": "Project-control action. Pause/resume only affect future dispatch."
			},
			"projectId": {
				"type": "string",
				"description": "Registered Decodex project id. Optional only when the gateway was started with a project config."
			},
			"authority": {
				"type": "object",
				"additionalProperties": false,
				"properties": {
					"reason": {
						"type": "string",
						"description": "Explicit operator reason for pause or resume."
					},
					"source": {
						"type": "string",
						"description": "Remote client or operator source identifier."
					},
					"acknowledgeFutureDispatchOnly": {
						"type": "boolean",
						"description": "Must be true for pause/resume; active lanes are not killed."
					}
				}
			}
		},
		"required": ["action"]
	})
}

pub(in crate::mcp) fn lane_control_tool_output_schema() -> Value {
	tool_output_schema(serde_json::json!({
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

pub(in crate::mcp) fn project_control_tool_output_schema() -> Value {
	tool_output_schema(serde_json::json!({
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
