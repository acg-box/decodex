use crate::orchestrator::{self, RepoGateFailure, tests};

#[test]
fn repo_gate_classifies_git_index_lock_contention_as_retryable_runtime_failure() {
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
	let repo_root = config.repo_root();
	let error = orchestrator::run_repo_gate_commands(
		&[String::from(
			"printf \"%s\\n\" \"fatal: Unable to create '.git/index.lock': File exists.\" >&2; exit 1",
		)],
		&[],
		repo_root,
	)
	.expect_err("git index.lock contention should fail the repo gate");
	let repo_gate_failure = error
		.downcast_ref::<RepoGateFailure>()
		.expect("repo gate failures should preserve structured classification");

	assert_eq!(repo_gate_failure.error_class(), "repo_gate_git_lock_contention");
	assert_eq!(
		repo_gate_failure.disposition(),
		orchestrator::RepoGateFailureDisposition::RetryAfterBackoff
	);
}
