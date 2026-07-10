use serde_json::{self, Value};

use crate::mcp::tool_schemas;

pub(in crate::mcp) fn autonomy_runtime_policy_acceptance_tool_output_schema() -> Value {
	tool_schemas::tool_output_schema(serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"schema": { "type": "string", "enum": ["decodex.mcp.autonomy_runtime_policy_acceptance_result/1"] },
			"status": { "type": "string", "enum": ["ok"] },
			"mode": { "type": "string", "enum": ["dry_run", "apply"] },
			"persisted": { "type": "boolean" },
			"project_id": { "type": "string" },
			"candidate_digest": { "type": "string" },
			"policy": {
				"type": "object",
				"additionalProperties": false,
				"properties": {
					"policy_id": { "type": "string" },
					"policy_version": { "type": "string" },
					"objective_id": { "type": "string" },
					"objective_version": { "type": "integer", "minimum": 1 },
					"authority_ref": { "type": "string" },
					"accepted_by": { "type": "string" },
					"accepted_at": { "type": "string" },
					"acceptance_source": { "type": "string" },
					"public_non_goals": { "type": "array", "minItems": 1, "items": { "type": "string" } }
				},
				"required": [
					"policy_id", "policy_version", "objective_id", "objective_version",
					"authority_ref", "accepted_by", "accepted_at", "acceptance_source",
					"public_non_goals"
				]
			},
			"authority_effect": { "type": "string" },
			"next_action": { "type": "string" }
		},
		"required": ["schema", "status", "mode", "persisted", "project_id", "candidate_digest", "policy", "authority_effect", "next_action"]
	}))
}

pub(in crate::mcp) fn autonomy_runtime_policy_apply_tool_output_schema() -> Value {
	tool_schemas::tool_output_schema(serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"schema": { "type": "string", "enum": ["decodex.mcp.autonomy_runtime_policy_apply_result/1"] },
			"status": { "type": "string", "enum": ["ok"] },
			"mode": { "type": "string", "enum": ["dry_run", "apply"] },
			"persisted": { "type": "boolean" },
			"project_id": { "type": "string" },
			"proposal_id": { "type": "string" },
			"decision_contract_id": { "type": "string" },
			"eligible": { "type": "boolean" },
			"objections": { "type": "array", "items": { "type": "string" } },
			"challenge_recorded": { "type": "boolean" },
			"execution_authority_granted": { "type": "boolean" },
			"program_intake_present": { "type": "boolean" },
			"program_intake_state": { "type": "string", "enum": ["absent", "partial", "complete", "inconsistent"] },
			"intake_team_issue_identifier": { "type": ["string", "null"] },
			"authority_effect": { "type": "string" },
			"next_action": { "type": "string" }
		},
		"required": [
			"schema", "status", "mode", "persisted", "project_id", "proposal_id",
			"decision_contract_id", "eligible", "objections", "challenge_recorded",
			"execution_authority_granted", "program_intake_present", "program_intake_state", "intake_team_issue_identifier", "authority_effect", "next_action"
		]
	}))
}
