use serde_json::{self, Value};

use crate::mcp::tool_schemas;

pub(in crate::mcp) fn observe_tool_input_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"issue": {
				"type": "string",
				"description": "Optional issue identifier or tracker id to inspect one lane."
			},
			"runId": {
				"type": "string",
				"description": "Optional run id used with issue-scoped lane inspection."
			},
			"limit": {
				"type": "integer",
				"minimum": 1,
				"description": "Maximum recent run count for project observability."
			}
		}
	})
}

pub(in crate::mcp) fn plan_tool_input_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"intent": {
				"type": "string",
				"enum": ["research", "validation_ready", "handoff", "lane_control"],
				"description": "Decodex workflow intent to route."
			},
			"issue": {
				"type": "string",
				"description": "Optional issue identifier for lane-scoped prompts."
			},
			"contractId": {
				"type": "string",
				"description": "Optional Decision Contract id for research or intake planning."
			}
		},
		"required": ["intent"]
	})
}

pub(in crate::mcp) fn research_compile_tool_input_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"mode": {
				"type": "string",
				"enum": ["dry_run", "apply"],
				"description": "dry_run validates without persistence; apply persists a latent Decision Contract."
			},
			"projectId": {
				"type": "string",
				"description": "Optional Decodex service id when the MCP context is not project-scoped."
			},
			"input": {
				"type": "object",
				"additionalProperties": true,
				"description": "Structured Decodex research/design input."
			},
			"intent": {
				"type": "string",
				"description": "Minimal natural-language research/design intent."
			},
			"sourceIssue": {
				"type": "string",
				"description": "Optional source tracker issue identifier for minimal intent intake."
			},
			"outcome": {
				"type": "string",
				"enum": ["decision_ready", "not_decision_ready", "blocked", "needs_human_decision"]
			},
			"authority": tool_schemas::planning_authority_input_schema()
		}
	})
}

pub(in crate::mcp) fn research_promote_tool_input_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"mode": {
				"type": "string",
				"enum": ["dry_run", "apply"],
				"description": "dry_run inspects readiness; apply records explicit acceptance."
			},
			"projectId": {
				"type": "string",
				"description": "Optional Decodex service id when the MCP context is not project-scoped."
			},
			"contractId": {
				"type": "string",
				"description": "Decision Contract identifier to inspect or promote."
			},
			"authority": tool_schemas::planning_authority_input_schema()
		},
		"required": ["contractId"]
	})
}

pub(in crate::mcp) fn intake_goal_tool_input_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"mode": {
				"type": "string",
				"enum": ["dry_run", "apply"],
				"description": "dry_run previews generated issues; apply materializes only with explicit authority."
			},
			"contractId": {
				"type": "string",
				"description": "Promoted Decision Contract identifier to materialize."
			},
			"teamIssueIdentifier": {
				"type": "string",
				"description": "Optional source issue used to anchor generated issue team/state on apply."
			},
			"authority": tool_schemas::planning_authority_input_schema()
		},
		"required": ["contractId"]
	})
}
