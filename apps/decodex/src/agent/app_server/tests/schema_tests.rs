use std::fs;

use tempfile::TempDir;

use crate::agent::app_server::{
	self, APP_SERVER_SCHEMA_REQUIRED_MARKERS, AppServerCapabilityPreflightReport,
	AppServerRunResult, tests,
};

#[test]
fn matches_thread_id_from_supported_notification_shapes() {
	for message in [
		tests::notification_message(
			"thread/started",
			serde_json::json!({
				"thread": {
					"id": "thread-1",
				}
			}),
		),
		tests::notification_message(
			"turn/completed",
			serde_json::json!({
				"threadId": "thread-1",
				"turn": {
					"id": "turn-1",
					"status": "completed",
					"error": null,
				}
			}),
		),
	] {
		assert!(app_server::targets_thread(&message, Some("thread-1")));
		assert!(!app_server::targets_thread(&message, Some("thread-2")));
	}
}

#[test]
fn probe_result_shape_is_stable() {
	let result = AppServerRunResult {
		user_agent: String::from("ua"),
		capability_preflight: AppServerCapabilityPreflightReport::new(),
		thread_id: String::from("thread"),
		turn_id: String::from("turn"),
		turn_count: 1,
		event_count: 3,
		final_output: String::from("PROBE_OK"),
		continuation_pending: false,
		phase_goal_status: None,
	};

	assert_eq!(result.final_output, "PROBE_OK");
	assert_eq!(result.turn_count, 1);
}

#[test]
fn generated_schema_marker_validation_accepts_required_markers() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let schema_path = temp_dir.path().join("app-server.schema.json");

	fs::write(
		&schema_path,
		serde_json::json!({
			"description": "Decodex app-server compatibility fixture.",
			"requiredMarkers": super::APP_SERVER_SCHEMA_REQUIRED_MARKERS,
			"title": "ThreadStartParams",
			"properties": {
				"dynamicTools": {
					"items": {
						"$ref": "#/definitions/DynamicToolSpec"
					}
				},
				"marketplaceKinds": { "type": "array" },
				"type": { "const": "inputText" }
			},
			"definitions": {
				"DynamicToolNamespaceTool": {
					"oneOf": [{
						"title": "FunctionDynamicToolNamespaceTool",
						"required": ["description", "inputSchema", "name", "type"],
						"properties": {
							"deferLoading": { "type": "boolean" },
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
								"deferLoading": { "type": "boolean" },
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
	tests::write_app_server_method_union_fixtures(temp_dir.path(), None);
	app_server::validate_generated_app_server_schema(temp_dir.path())
		.expect("required markers should pass schema validation");
}

#[test]
fn generated_schema_marker_validation_rejects_legacy_flat_dynamic_tools() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let schema_path = temp_dir.path().join("app-server.schema.json");

	fs::write(
		&schema_path,
		serde_json::json!({
			"requiredMarkers": super::APP_SERVER_SCHEMA_REQUIRED_MARKERS,
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

#[test]
fn generated_schema_marker_validation_rejects_missing_owned_method() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let schema_path = temp_dir.path().join("app-server.schema.json");

	fs::write(
		&schema_path,
		serde_json::json!({
			"requiredMarkers": super::APP_SERVER_SCHEMA_REQUIRED_MARKERS,
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

#[test]
fn generated_schema_marker_validation_rejects_missing_markers() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let schema_path = temp_dir.path().join("app-server.schema.json");

	fs::write(
		&schema_path,
		serde_json::json!({
			"methods": ["initialize"]
		})
		.to_string(),
	)
	.expect("schema fixture should write");

	let error = app_server::validate_generated_app_server_schema(temp_dir.path())
		.expect_err("missing markers should fail schema validation");

	assert!(error.to_string().contains("missing required Decodex markers"));
	assert!(error.to_string().contains("turn/start"));
	assert!(error.to_string().contains("marketplaceKinds"));
}

#[test]
fn generated_schema_marker_validation_rejects_prose_only_markers() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let schema_path = temp_dir.path().join("app-server.schema.json");
	let prose_markers = APP_SERVER_SCHEMA_REQUIRED_MARKERS.join(", ");

	fs::write(
		&schema_path,
		serde_json::json!({
			"description": prose_markers.clone(),
			"$comment": "Compatibility prose, not protocol structure.",
			"properties": {
				"documentationOnly": {
					"description": prose_markers
				}
			}
		})
		.to_string(),
	)
	.expect("schema fixture should write");

	let error = app_server::validate_generated_app_server_schema(temp_dir.path())
		.expect_err("prose-only markers should fail schema validation");

	assert!(error.to_string().contains("missing required Decodex markers"));
	assert!(error.to_string().contains("initialize"));
	assert!(error.to_string().contains("marketplaceKinds"));
}
