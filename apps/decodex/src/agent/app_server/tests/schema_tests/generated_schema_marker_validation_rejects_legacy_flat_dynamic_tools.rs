use std::fs;

use tempfile::TempDir;

use crate::agent::app_server::{self};

#[test]
fn generated_schema_marker_validation_rejects_legacy_flat_dynamic_tools() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let schema_path = temp_dir.path().join("app-server.schema.json");

	fs::write(
		&schema_path,
		serde_json::json!({
			"requiredMarkers": crate::agent::app_server::APP_SERVER_SCHEMA_REQUIRED_MARKERS,
			"title": "ThreadStartParams",
			"properties": {
				"dynamicTools": {
					"items": {
						"$ref": "#/definitions/DynamicToolSpec"
					}
				}
			},
			"definitions": {
				"DynamicToolSpec": {
					"type": "object",
					"required": ["description", "inputSchema", "name"],
					"properties": {
						"deferLoading": { "type": "boolean" },
						"description": { "type": "string" },
						"inputSchema": true,
						"name": { "type": "string" },
						"namespace": { "type": ["string", "null"] }
					}
				}
			}
		})
		.to_string(),
	)
	.expect("schema fixture should write");

	let error = app_server::validate_generated_app_server_schema(temp_dir.path())
		.expect_err("legacy flat dynamicTools should fail schema validation");

	assert!(error.to_string().contains("0.141 dynamicTools tagged union"));
}
