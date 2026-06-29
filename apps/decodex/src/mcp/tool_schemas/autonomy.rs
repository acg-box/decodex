use serde_json::{self, Value};

use super::{planning_authority_input_schema, tool_output_schema};

pub(in crate::mcp) fn autonomy_draft_objective_tool_input_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"mode": {
				"type": "string",
				"enum": ["dry_run", "apply"],
				"description": "dry_run validates the Objective Contract; apply persists a draft only."
			},
			"projectId": {
				"type": "string",
				"description": "Optional Decodex service id when the MCP context is not project-scoped."
			},
			"objective": {
				"type": "object",
				"additionalProperties": true,
				"description": "decodex.autonomy_objective/1 payload with state=draft."
			},
			"authority": planning_authority_input_schema()
		},
		"required": ["objective"]
	})
}

pub(in crate::mcp) fn autonomy_accept_objective_tool_input_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"mode": {
				"type": "string",
				"enum": ["dry_run", "apply"],
				"description": "dry_run inspects the draft acceptance target; apply accepts the draft Objective Contract version."
			},
			"projectId": {
				"type": "string",
				"description": "Optional Decodex service id when the MCP context is not project-scoped."
			},
			"objectiveId": {
				"type": "string",
				"description": "Objective Contract id to accept."
			},
			"objectiveVersion": {
				"type": "integer",
				"minimum": 1,
				"description": "Draft Objective Contract version to accept."
			},
			"authority": {
				"type": "object",
				"additionalProperties": false,
				"description": "Explicit human/operator objective acceptance authority. Runtime-policy acceptance requires trusted Decodex state and is not accepted from caller-supplied fields.",
				"properties": {
					"acceptedBy": {
						"type": "string",
						"description": "Human or operator actor accepting the Objective Contract."
					},
					"acceptedByKind": {
						"type": "string",
						"enum": ["user"],
						"description": "Only direct user/operator acceptance is accepted through this tool until trusted runtime-policy resolution exists."
					},
					"acceptedAt": {
						"type": "string",
						"description": "Optional RFC3339 acceptance timestamp; Decodex fills the current time when omitted."
					},
					"acceptanceSource": {
						"type": "string",
						"description": "Source of the explicit acceptance, such as conversation or operator command."
					}
				},
				"required": ["acceptedBy", "acceptedByKind", "acceptanceSource"]
			}
		},
		"required": ["objectiveId", "objectiveVersion"]
	})
}

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
					"docs_skill_drift"
				]
			},
			"signal": {
				"type": "object",
				"additionalProperties": true,
				"description": "Signal input without derived id/fingerprint; Decodex derives stable identity."
			},
			"authority": planning_authority_input_schema()
		},
		"required": ["kind", "signal"]
	})
}

pub(in crate::mcp) fn autonomy_compile_proposal_tool_input_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"mode": {
				"type": "string",
				"enum": ["dry_run", "apply"],
				"description": "dry_run compiles non-executable proposal evidence; apply persists it."
			},
			"projectId": {
				"type": "string",
				"description": "Optional Decodex service id when the MCP context is not project-scoped."
			},
			"proposal": {
				"type": "object",
				"additionalProperties": true,
				"description": "Autonomy proposal compile input."
			},
			"signalIds": {
				"type": "array",
				"items": { "type": "string" },
				"description": "Persisted autonomy signal ids to bind into the proposal."
			},
			"authority": planning_authority_input_schema()
		},
		"required": ["proposal"]
	})
}

pub(in crate::mcp) fn autonomy_challenge_proposal_tool_input_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"mode": {
				"type": "string",
				"enum": ["dry_run", "apply"],
				"description": "dry_run previews the challenge effect; apply records challenge evidence."
			},
			"projectId": {
				"type": "string",
				"description": "Optional Decodex service id when the MCP context is not project-scoped."
			},
			"proposalId": {
				"type": "string",
				"description": "Stable autonomy proposal id."
			},
			"challenge": {
				"type": "object",
				"additionalProperties": true,
				"description": "Challenge evidence. It is not acceptance authority."
			},
			"authority": planning_authority_input_schema()
		},
		"required": ["proposalId", "challenge"]
	})
}

pub(in crate::mcp) fn autonomy_request_promotion_tool_input_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"mode": {
				"type": "string",
				"enum": ["dry_run", "apply"],
				"description": "dry_run explains required authority; apply creates a latent Decision Contract candidate only with explicit proposal acceptance authority."
			},
			"projectId": {
				"type": "string",
				"description": "Optional Decodex service id when the MCP context is not project-scoped."
			},
			"proposalId": {
				"type": "string",
				"description": "Stable autonomy proposal id."
			},
			"authority": {
				"type": "object",
				"additionalProperties": true,
				"description": "Explicit proposal acceptance authority, including acceptedBy, acceptedByKind, acceptanceSource, reason, proposalActor, and proposalActorKind. acceptedProjectPolicy payloads are refused because trusted policy authority must be resolved from Decodex state."
			}
		},
		"required": ["proposalId"]
	})
}

pub(in crate::mcp) fn autonomy_objective_tool_output_schema() -> Value {
	tool_output_schema(serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"schema": { "type": "string", "enum": ["decodex.mcp.autonomy_objective_result/1"] },
			"status": { "type": "string", "enum": ["ok"] },
			"mode": { "type": "string", "enum": ["dry_run", "apply"] },
			"persisted": { "type": "boolean" },
			"project_id": { "type": "string" },
			"objective": { "type": "object", "additionalProperties": true },
			"authority_effect": { "type": "string" },
			"next_action": { "type": "string" },
			"updated_at": { "type": ["string", "null"] }
		},
		"required": [
			"schema",
			"status",
			"mode",
			"persisted",
			"project_id",
			"objective",
			"authority_effect",
			"next_action"
		]
	}))
}

pub(in crate::mcp) fn autonomy_signal_tool_output_schema() -> Value {
	tool_output_schema(serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"schema": { "type": "string", "enum": ["decodex.mcp.autonomy_signal_result/1"] },
			"status": { "type": "string", "enum": ["ok"] },
			"mode": { "type": "string", "enum": ["dry_run", "apply"] },
			"persisted": { "type": "boolean" },
			"project_id": { "type": "string" },
			"signal": { "type": "object", "additionalProperties": true },
			"authority_effect": { "type": "string" },
			"next_action": { "type": "string" },
			"updated_at": { "type": ["string", "null"] }
		},
		"required": [
			"schema",
			"status",
			"mode",
			"persisted",
			"project_id",
			"signal",
			"authority_effect",
			"next_action"
		]
	}))
}

pub(in crate::mcp) fn autonomy_proposal_tool_output_schema() -> Value {
	tool_output_schema(serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"schema": { "type": "string", "enum": ["decodex.mcp.autonomy_proposal_result/1"] },
			"status": { "type": "string", "enum": ["ok"] },
			"mode": { "type": "string", "enum": ["dry_run", "apply"] },
			"persisted": { "type": "boolean" },
			"project_id": { "type": "string" },
			"proposal": { "type": "object", "additionalProperties": true },
			"authority_effect": { "type": "string" },
			"next_action": { "type": "string" },
			"updated_at": { "type": ["string", "null"] }
		},
		"required": [
			"schema",
			"status",
			"mode",
			"persisted",
			"project_id",
			"proposal",
			"authority_effect",
			"next_action"
		]
	}))
}

pub(in crate::mcp) fn autonomy_challenge_tool_output_schema() -> Value {
	tool_output_schema(serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"schema": { "type": "string", "enum": ["decodex.mcp.autonomy_challenge_result/1"] },
			"status": { "type": "string", "enum": ["ok"] },
			"mode": { "type": "string", "enum": ["dry_run", "apply"] },
			"persisted": { "type": "boolean" },
			"project_id": { "type": "string" },
			"proposal": { "type": "object", "additionalProperties": true },
			"challenge_evidence_count": { "type": "integer", "minimum": 0 },
			"authority_effect": { "type": "string" },
			"next_action": { "type": "string" },
			"updated_at": { "type": ["string", "null"] }
		},
		"required": [
			"schema",
			"status",
			"mode",
			"persisted",
			"project_id",
			"proposal",
			"challenge_evidence_count",
			"authority_effect",
			"next_action"
		]
	}))
}

pub(in crate::mcp) fn autonomy_promotion_request_tool_output_schema() -> Value {
	tool_output_schema(serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"schema": { "type": "string", "enum": ["decodex.mcp.autonomy_promotion_request_result/1"] },
			"status": { "type": "string", "enum": ["ok"] },
			"mode": { "type": "string", "enum": ["dry_run", "apply"] },
			"persisted": { "type": "boolean" },
			"project_id": { "type": "string" },
			"proposal": { "type": "object", "additionalProperties": true },
			"decision_contract_id": { "type": ["string", "null"] },
			"execution_authority_granted": { "type": "boolean" },
			"required_authority": { "type": "array", "items": { "type": "string" } },
			"authority_effect": { "type": "string" },
			"next_action": { "type": "string" }
		},
		"required": [
			"schema",
			"status",
			"mode",
			"persisted",
			"project_id",
			"proposal",
			"execution_authority_granted",
			"required_authority",
			"authority_effect",
			"next_action"
		]
	}))
}
