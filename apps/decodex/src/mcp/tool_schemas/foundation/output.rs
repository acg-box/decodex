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
				"enum": ["decodex.mcp.plan_result/1"]
			},
			"status": {
				"type": "string",
				"enum": ["ok"]
			},
			"intent": {
				"type": "string",
				"enum": ["research", "validation_ready", "handoff", "lane_control"]
			},
			"prompt": {
				"type": "string"
			},
			"resource": {
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
		"required": ["schema", "status", "intent", "prompt", "resource", "next_action"]
	}))
}

pub(in crate::mcp) fn research_compile_tool_output_schema() -> Value {
	tool_schemas::tool_output_schema(serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"schema": { "type": "string", "enum": ["decodex.mcp.research_compile_result/1"] },
			"status": { "type": "string", "enum": ["ok"] },
			"mode": { "type": "string", "enum": ["dry_run", "apply"] },
			"persisted": { "type": "boolean" },
			"contract_id": { "type": "string" },
			"contract_status": {
				"type": "string",
				"enum": ["draft_latent", "accepted_promoted", "rejected_superseded", "needs_human_decision"]
			},
			"ready_for_issue_shaping": { "type": "boolean" },
			"issue_generation_ready_after_promotion": { "type": "boolean" },
			"execution_authority_granted": { "type": "boolean" },
			"proposed_issue_count": { "type": "integer", "minimum": 0 },
			"promotion_targets": { "type": "array", "items": { "type": "string" } },
			"conflict_domains": { "type": "array", "items": { "type": "string" } },
			"next_action": { "type": "string" }
		},
		"required": [
			"schema",
			"status",
			"mode",
			"persisted",
			"contract_id",
			"contract_status",
			"ready_for_issue_shaping",
			"issue_generation_ready_after_promotion",
			"execution_authority_granted",
			"proposed_issue_count",
			"promotion_targets",
			"conflict_domains",
			"next_action"
		]
	}))
}

pub(in crate::mcp) fn research_promote_tool_output_schema() -> Value {
	tool_schemas::tool_output_schema(serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"schema": { "type": "string", "enum": ["decodex.mcp.research_promote_result/1"] },
			"status": { "type": "string", "enum": ["ok"] },
			"mode": { "type": "string", "enum": ["dry_run", "apply"] },
			"persisted": { "type": "boolean" },
			"contract_id": { "type": "string" },
			"contract_status": {
				"type": "string",
				"enum": ["draft_latent", "accepted_promoted", "rejected_superseded", "needs_human_decision"]
			},
			"execution_authority_granted": { "type": "boolean" },
			"ready_for_issue_shaping": { "type": "boolean" },
			"next_action": { "type": "string" }
		},
		"required": [
			"schema",
			"status",
			"mode",
			"persisted",
			"contract_id",
			"contract_status",
			"execution_authority_granted",
			"ready_for_issue_shaping",
			"next_action"
		]
	}))
}

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
