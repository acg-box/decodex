use std::fs;

use crate::orchestrator::{self, RepoGateFailure, tests};

#[test]
fn repo_gate_stops_canonicalize_failure_after_scope_envelope_widening() {
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
	let repo_root = config.repo_root();

	tests::commit_worktree_change(repo_root, "owned.txt", "before\n", "add owned file");
	tests::commit_worktree_change(repo_root, "outside.txt", "before\n", "add outside file");
	fs::write(repo_root.join("owned.txt"), "implementation\n")
		.expect("pre-gate implementation diff should write");

	let error = orchestrator::run_repo_gate_commands(
		&[String::from("printf 'rewritten\\n' > outside.txt; exit 1")],
		&[],
		repo_root,
	)
	.expect_err("canonicalize widening should stop before ordinary repair retry");
	let repo_gate_failure = error
		.downcast_ref::<RepoGateFailure>()
		.expect("scope envelope violation should preserve repo-gate classification");

	assert_eq!(repo_gate_failure.error_class(), "repo_gate_scope_envelope_violation");
	assert_eq!(
		repo_gate_failure.disposition(),
		orchestrator::RepoGateFailureDisposition::NeedsHumanAttention
	);
	assert!(
		error.to_string().contains("outside.txt"),
		"error should name the out-of-scope rewrite"
	);
}
