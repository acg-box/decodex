use crate::orchestrator::{self, RepoGateFailure, tests};

#[test]
fn repo_gate_preflight_rejects_missing_cwd_before_commands() {
	let (temp_dir, config, _workflow) = tests::temp_project_layout();
	let missing_worktree = config.repo_root().join("missing-worktree");
	drop(temp_dir);

	let error = orchestrator::run_repo_gate_commands(
		&["printf should-not-run > side-effect.txt".to_owned()],
		&[],
		&missing_worktree,
	)
	.expect_err("missing cwd should fail before repo-gate command execution");
	let repo_gate_failure = error
		.downcast_ref::<RepoGateFailure>()
		.expect("repo-gate preflight should preserve structured failure");

	assert_eq!(repo_gate_failure.error_class(), "repo_gate_command_spawn_failed");
	assert_eq!(
		repo_gate_failure.disposition(),
		orchestrator::RepoGateFailureDisposition::NeedsHumanAttention
	);
	assert!(error.to_string().contains("command preflight failed"));
}
