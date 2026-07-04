use serde_json::{Map, Value};

use crate::agent::tracker_tool_bridge::tools::review_checkpoint::schema;

pub(in crate::agent::tracker_tool_bridge::tools) fn review_checkpoint_findings_array_schema(
	rejected: bool,
) -> Value {
	serde_json::json!({
		"type": "array",
		"items": review_checkpoint_finding_schema(rejected)
	})
}

fn review_checkpoint_finding_schema(rejected: bool) -> Value {
	let mut properties = Map::from_iter([
		(String::from("severity"), review_checkpoint_severity_schema()),
		(String::from("summary"), serde_json::json!({ "type": "string" })),
		(String::from("evidence"), schema::non_empty_string_array_schema()),
		(String::from("kind"), serde_json::json!({ "type": "string" })),
		(String::from("file"), serde_json::json!({ "type": "string" })),
		(String::from("line"), serde_json::json!({ "type": "integer", "minimum": 1 })),
		(String::from("line_range"), review_checkpoint_line_range_schema()),
	]);
	let required = if rejected {
		properties
			.insert(String::from("rejection_reason"), serde_json::json!({ "type": "string" }));

		serde_json::json!(["severity", "summary", "rejection_reason", "evidence"])
	} else {
		properties.insert(String::from("guidance"), serde_json::json!({ "type": "string" }));

		serde_json::json!(["severity", "summary", "evidence", "guidance"])
	};

	serde_json::json!({
		"type": "object",
		"properties": properties,
		"required": required,
		"additionalProperties": false
	})
}

fn review_checkpoint_severity_schema() -> Value {
	serde_json::json!({
		"type": "string",
		"enum": ["critical", "high", "medium", "low", "info"]
	})
}

fn review_checkpoint_line_range_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"properties": {
			"start": { "type": "integer", "minimum": 1 },
			"end": { "type": "integer", "minimum": 1 }
		},
		"required": ["start", "end"],
		"additionalProperties": false
	})
}
