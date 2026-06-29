use serde_json::{self, Value};

use crate::mcp::tool_schemas;

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
			"authority": tool_schemas::planning_authority_input_schema()
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
