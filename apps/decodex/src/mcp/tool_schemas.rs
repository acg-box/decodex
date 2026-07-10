mod autonomy;
mod foundation;
mod operator;

pub(super) use self::{
	autonomy::{
		autonomy_accept_objective_tool_input_schema,
		autonomy_accept_runtime_policy_tool_input_schema,
		autonomy_apply_runtime_policy_tool_input_schema,
		autonomy_challenge_proposal_tool_input_schema, autonomy_challenge_tool_output_schema,
		autonomy_compile_proposal_tool_input_schema, autonomy_draft_objective_tool_input_schema,
		autonomy_objective_tool_output_schema, autonomy_promotion_request_tool_output_schema,
		autonomy_proposal_tool_output_schema, autonomy_request_promotion_tool_input_schema,
		autonomy_runtime_policy_acceptance_tool_output_schema,
		autonomy_runtime_policy_apply_tool_output_schema, autonomy_signal_tool_output_schema,
		autonomy_submit_signal_tool_input_schema,
	},
	foundation::{
		intake_goal_tool_input_schema, intake_goal_tool_output_schema, observe_tool_input_schema,
		observe_tool_output_schema, plan_tool_input_schema, plan_tool_output_schema,
	},
	operator::{
		lane_control_tool_input_schema, lane_control_tool_output_schema,
		project_control_tool_input_schema, project_control_tool_output_schema,
	},
};

use serde_json::{self, Value};

fn planning_authority_input_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"source": {
				"type": "string",
				"description": "Explicit remote client or operator source for an apply-style call."
			},
			"reason": {
				"type": "string",
				"description": "Explicit reason authorizing an apply-style call."
			},
			"runId": {
				"type": "string",
				"description": "Current lane run id when a future planning mutation is lane-scoped."
			},
			"expectedTurnId": {
				"type": "string",
				"description": "Current lane turn id when a future planning mutation is lane-scoped."
			}
		}
	})
}

fn tool_output_schema(primary_schema: Value) -> Value {
	serde_json::json!({
		"oneOf": [
			primary_schema,
			tool_refusal_output_schema(),
			tool_validation_error_output_schema()
		]
	})
}

fn tool_refusal_output_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"schema": {
				"type": "string",
				"enum": ["decodex.mcp.refusal/1"]
			},
			"status": {
				"type": "string",
				"enum": ["refused"]
			},
			"reason": {
				"type": "string"
			},
			"message": {
				"type": "string"
			},
			"tool": {
				"type": "string"
			},
			"capability_profile": {
				"type": "string",
				"enum": ["observe", "plan", "operate", "admin"]
			},
			"required_capability_profile": {
				"type": "string",
				"enum": ["observe", "plan", "operate", "admin"]
			}
		},
		"required": ["schema", "status", "reason", "message"]
	})
}

fn tool_validation_error_output_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"schema": {
				"type": "string",
				"enum": ["decodex.mcp.tool_validation_error/1"]
			},
			"status": {
				"type": "string",
				"enum": ["refused"]
			},
			"reason": {
				"type": "string",
				"enum": ["invalid_arguments"]
			},
			"tool": {
				"type": "string"
			},
			"message": {
				"type": "string"
			}
		},
		"required": ["schema", "status", "reason", "tool", "message"]
	})
}
