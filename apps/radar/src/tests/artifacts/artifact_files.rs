use std::fs;

use serde_json::Value;

use crate::{
	RadarRenderSignalRequest, RadarValidateRequest,
	tests::{env::TestEnvVars, fixtures},
};

#[test]
fn default_github_token_falls_back_to_workflow_token() {
	let _env = TestEnvVars::set(&[
		("GITHUB_PAT_X", Some("")),
		("GITHUB_PAT_Y", Some("")),
		("GITHUB_TOKEN", Some("workflow-token")),
	]);

	assert_eq!(crate::github_token(None).as_deref(), Some("workflow-token"));
}

#[test]
fn explicit_github_token_env_does_not_fall_back_to_workflow_token() {
	let _env = TestEnvVars::set(&[
		("DECODEX_TEST_MISSING_RADAR_TOKEN", None),
		("GITHUB_TOKEN", Some("workflow-token")),
	]);

	assert_eq!(crate::github_token(Some("DECODEX_TEST_MISSING_RADAR_TOKEN")), None);
}

#[test]
fn validates_json_files_from_directory() {
	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let path = temp_dir.path().join("bundle.json");

	fs::write(&path, fixtures::valid_bundle().to_string()).expect("fixture should be written");

	let report =
		crate::validate(&RadarValidateRequest { paths: vec![temp_dir.path().to_path_buf()] })
			.expect("valid temporary bundle should pass");

	assert_eq!(report.checked_files, 1);
}

#[test]
fn renders_signal_from_bundle_and_analysis_fixture() {
	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let bundle_path = temp_dir.path().join("bundle.json");
	let analysis_path = temp_dir.path().join("analysis.json");
	let signal_path = temp_dir.path().join("signal.json");
	let analysis = serde_json::json!({
		"kind": "capability",
		"title": "Unix sockets for remote Codex",
		"summary": "Remote Codex can use Unix socket endpoints.",
		"why_it_matters": "Operators can use local socket transports.",
		"confidence": "confirmed",
		"impact": "medium",
		"proof_points": ["PR #22414 adds endpoint handling."],
		"slug": null,
		"config_flags": [],
		"how_to_try": null,
		"expected_effect": null,
		"caveats": null,
		"watch_state": null
	});

	fs::write(&bundle_path, fixtures::valid_bundle().to_string())
		.expect("bundle should be written");
	fs::write(&analysis_path, analysis.to_string()).expect("analysis should be written");

	let report = crate::render_signal(&RadarRenderSignalRequest {
		bundle: bundle_path,
		analysis: analysis_path,
		out: signal_path.clone(),
		published_at: None,
	})
	.expect("rendered signal should pass validation");
	let rendered: Value = serde_json::from_str(
		&fs::read_to_string(&signal_path).expect("rendered signal should be readable"),
	)
	.expect("rendered signal should parse");

	assert_eq!(report.out, signal_path);
	assert_eq!(rendered["schema"], "signal_entry/v1");
	assert_eq!(rendered["slug"], "unix-sockets-for-remote-codex");
	assert_eq!(rendered["published_at"], "2026-06-01T00:00:00Z");
	assert_eq!(rendered["source_refs"]["items"][0]["meta"], serde_json::json!("#22414"));
	assert_eq!(rendered["source_refs"]["items"][1]["meta"], "abc123");
	assert!(rendered.get("how_to_try").is_none());
}
