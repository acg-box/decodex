use serde_json::{self, Value};

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
