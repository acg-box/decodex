use std::ffi::OsStr;

use crate::orchestrator::{self, RepoGateFailure, tests};

#[test]
fn repo_gate_cleanliness_check_spawn_failures_require_human_attention() {
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
	let repo_root = config.repo_root();
	let error = orchestrator::run_repo_gate_cleanliness_check_with_git(
		OsStr::new("/definitely-missing-git-for-tests"),
		repo_root,
	)
	.expect_err("missing git binary should preserve repo gate classification");
	let repo_gate_failure = error
		.downcast_ref::<RepoGateFailure>()
		.expect("repo gate failures should preserve structured classification");

	assert!(error.to_string().contains("tracked-file cleanliness check"));
	assert_eq!(repo_gate_failure.error_class(), "repo_gate_command_spawn_failed");
	assert_eq!(
		repo_gate_failure.disposition(),
		orchestrator::RepoGateFailureDisposition::NeedsHumanAttention
	);
}
