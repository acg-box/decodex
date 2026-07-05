use serde_json::{self, Value};

use crate::mcp::{tool_schemas, tool_schemas::autonomy::input::proposal::schema};

pub(in crate::mcp) fn autonomy_compile_proposal_tool_input_schema() -> Value {
	let mut proposal_schema = schema::autonomy_compile_proposal_payload_schema();

	if let Some(object) = proposal_schema.as_object_mut() {
		object.insert(
			"description".to_owned(),
			Value::String("Autonomy proposal compile input.".to_owned()),
		);
	}

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
			"proposal": proposal_schema,
			"signalIds": {
				"type": "array",
				"items": { "type": "string" },
				"description": "Persisted autonomy signal ids to bind into the proposal."
			},
			"authority": tool_schemas::planning_authority_input_schema()
		},
		"required": ["proposal"]
	})
}
