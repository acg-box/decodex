use std::fs;

use tempfile::TempDir;

use crate::agent::app_server::{self};

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
