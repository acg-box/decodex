use std::process::{Command, Output};

use crate::github;

#[test]
fn generic_github_not_found_is_not_idempotent_cleanup() {
	let output = Output {
		status: Command::new("sh")
			.args(["-c", "exit 1"])
			.status()
			.expect("status command should run"),
		stdout: Vec::new(),
		stderr: b"gh: Not Found (HTTP 404)".to_vec(),
	};

	assert!(!github::gh_delete_ref_missing_branch(&output));
}
