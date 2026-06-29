use serde_json::{self, Value};

use crate::mcp::tool_schemas;

pub(in crate::mcp) fn autonomy_proposal_tool_output_schema() -> Value {
	tool_schemas::tool_output_schema(serde_json::json!({
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
	tool_schemas::tool_output_schema(serde_json::json!({
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
	tool_schemas::tool_output_schema(serde_json::json!({
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
