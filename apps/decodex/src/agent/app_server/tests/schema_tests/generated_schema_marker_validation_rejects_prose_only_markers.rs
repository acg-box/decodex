use std::fs;

use tempfile::TempDir;

use crate::agent::app_server::{self, APP_SERVER_SCHEMA_REQUIRED_MARKERS};

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
