use std::fs;

use crate::orchestrator::{self, RepoGateFailure, tests};

#[test]
fn repo_gate_rejects_dirty_tracked_files_left_by_canonicalize_commands() {
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
	let repo_root = config.repo_root();

	tests::commit_worktree_change(repo_root, "tracked.txt", "before\n", "add tracked file");

	let error = orchestrator::run_repo_gate_commands(
		&[String::from("printf 'after\\n' > tracked.txt")],
		&[String::from("grep -qx 'after' tracked.txt")],
		repo_root,
	)
	.expect_err("tracked autofix rewrites should fail the repo gate");
	let tracked_contents = fs::read_to_string(repo_root.join("tracked.txt"))
		.expect("tracked file should remain readable");
	let tracked_status =
		tests::git_output(repo_root, &["status", "--porcelain", "--untracked-files=no"]);
	let repo_gate_failure = error
		.downcast_ref::<RepoGateFailure>()
		.expect("repo gate failures should preserve structured classification");

	assert!(error.to_string().contains("verification"));
	assert_eq!(
		repo_gate_failure.error_class(),
		"repo_gate_lane_external_tracked_rewrite"
	);
	assert_eq!(
		repo_gate_failure.disposition(),
		orchestrator::RepoGateFailureDisposition::NeedsHumanAttention
	);
	let decision = repo_gate_failure
		.tracked_rewrite_decision()
		.expect("lane-external tracked rewrite should include rewrite evidence");
	assert_eq!(decision.to_json()["classification"], "lane_external_tracked_rewrite");
	assert_eq!(decision.to_json()["decision"], "require_scoped_authority");
	assert_eq!(decision.to_json()["fileCount"], 1);
	assert_eq!(tracked_contents, "after\n");
	assert!(tracked_status.contains("tracked.txt"));
}
