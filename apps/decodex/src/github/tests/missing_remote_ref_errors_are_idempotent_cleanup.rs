use std::process::{Command, Output};

use crate::github;

#[test]
fn missing_remote_ref_errors_are_idempotent_cleanup() {
	let output = Output {
		status: Command::new("sh")
			.args(["-c", "exit 1"])
			.status()
			.expect("status command should run"),
		stdout: Vec::new(),
		stderr: b"gh: Reference does not exist (HTTP 422)".to_vec(),
	};

	assert!(github::gh_delete_ref_missing_branch(&output));
}
