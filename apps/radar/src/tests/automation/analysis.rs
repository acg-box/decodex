use std::{path::Path, process::Command};

use crate::{RUN_CODEX_ANALYSIS_SCRIPT, tests::env::TestEnvVars};

#[test]
fn analysis_helper_fails_closed_without_explicit_boundary_opt_in() {
	let _env = TestEnvVars::set(&[("DECODEX_ALLOW_CODEX_ANALYSIS", None)]);
	let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
		.parent()
		.and_then(Path::parent)
		.expect("apps/decodex should live two levels under the repo root");
	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let bundle_path = temp_dir.path().join("missing-bundle.json");
	let output = Command::new("python3")
		.current_dir(repo_root)
		.arg(repo_root.join(RUN_CODEX_ANALYSIS_SCRIPT))
		.arg("--bundle")
		.arg(&bundle_path)
		.arg("--repo-root")
		.arg(repo_root)
		.output()
		.expect("Python analysis helper smoke command should execute");
	let stderr = String::from_utf8_lossy(&output.stderr);

	assert!(!output.status.success());
	assert!(
		stderr.contains("requires --allow-ai-analysis-boundary"),
		"unexpected stderr: {stderr}"
	);
}
