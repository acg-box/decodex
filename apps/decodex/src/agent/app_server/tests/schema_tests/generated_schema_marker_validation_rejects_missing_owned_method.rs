use std::fs;

use tempfile::TempDir;

use crate::agent::app_server::{self, tests};

#[test]
fn generated_schema_marker_validation_rejects_missing_owned_method() {
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
				"DynamicToolNamespaceTool": {
					"oneOf": [{
						"title": "FunctionDynamicToolNamespaceTool",
						"required": ["description", "inputSchema", "name", "type"],
						"properties": {
							"description": { "type": "string" },
							"inputSchema": true,
							"name": { "type": "string" },
							"type": { "enum": ["function"] }
						}
					}]
				},
				"DynamicToolSpec": {
					"oneOf": [
						{
							"title": "FunctionDynamicToolSpec",
							"required": ["description", "inputSchema", "name", "type"],
							"properties": {
								"description": { "type": "string" },
								"inputSchema": true,
								"name": { "type": "string" },
								"type": { "enum": ["function"] }
							}
						},
						{
							"title": "NamespaceDynamicToolSpec",
							"required": ["description", "name", "tools", "type"],
							"properties": {
								"description": { "type": "string" },
								"name": { "type": "string" },
								"tools": {
									"items": {
										"$ref": "#/definitions/DynamicToolNamespaceTool"
									}
								},
								"type": { "enum": ["namespace"] }
							}
						}
					]
				}
			}
		})
		.to_string(),
	)
	.expect("schema fixture should write");
	tests::write_app_server_method_union_fixtures(
		temp_dir.path(),
		Some(("ClientRequest", "turn/start")),
	);

	let error = app_server::validate_generated_app_server_schema(temp_dir.path())
		.expect_err("missing Decodex-owned method should fail schema validation");

	assert!(error.to_string().contains("ClientRequest"));
	assert!(error.to_string().contains("turn/start missing"));
}
