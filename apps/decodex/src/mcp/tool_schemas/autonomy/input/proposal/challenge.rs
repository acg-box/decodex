use serde_json::{self, Value};

use crate::mcp::tool_schemas;

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
			"authority": tool_schemas::planning_authority_input_schema()
		},
		"required": ["proposalId", "challenge"]
	})
}
