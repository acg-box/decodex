use crate::{
	orchestrator::{ServiceConfig, StateStore, tests, tests::ReviewPolicyCheckpointInput},
	test_support,
	tracker::TrackerIssue,
	worktree::{WorktreeManager, WorktreeSpec},
};

pub(super) fn retained_worktree_with_stale_review_lifecycle_authority(
	config: &ServiceConfig,
	state_store: &StateStore,
	issue: &TrackerIssue,
	pr_url: &str,
) -> (WorktreeSpec, String) {
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree =
		worktree_manager.ensure_worktree(&issue.identifier, false).expect("worktree should exist");
	let old_head_oid = git_head_oid_for_worktree(&worktree);

	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree should record");

	tests::seed_review_lifecycle_transition_fixture_for_path(
		state_store,
		config.service_id(),
		&worktree.path,
		&tests::sample_review_lifecycle_transition_fixture(
			&worktree.branch_name,
			pr_url,
			&old_head_oid,
			"waiting_for_result",
			1,
		),
	);

	let repaired_head_oid = commit_empty_repair_head_for_worktree(&worktree);

	(worktree, repaired_head_oid)
}

pub(super) fn git_head_oid_for_worktree(worktree: &WorktreeSpec) -> String {
	String::from_utf8(
		test_support::hermetic_git_command()
			.arg("-C")
			.arg(&worktree.path)
			.args(["rev-parse", "HEAD"])
			.output()
			.expect("git rev-parse should run")
			.stdout,
	)
	.expect("git output should be utf-8")
	.trim()
	.to_owned()
}

pub(super) fn commit_empty_repair_head_for_worktree(worktree: &WorktreeSpec) -> String {
	assert!(
		test_support::hermetic_git_command()
			.arg("-C")
			.arg(&worktree.path)
			.args([
				"-c",
				"user.name=Decodex Test",
				"-c",
				"user.email=decodex-test@example.invalid",
				"commit",
				"--allow-empty",
				"-m",
				"repair head",
			])
			.status()
			.expect("git commit should run")
			.success()
	);

	git_head_oid_for_worktree(worktree)
}

pub(super) fn seed_clean_repair_completion_writeback_gap(
	config: &ServiceConfig,
	state_store: &StateStore,
	issue: &TrackerIssue,
	worktree: &WorktreeSpec,
	pr_url: &str,
	repaired_head_oid: &str,
) {
	state_store
		.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
			project_id: config.service_id(),
			issue_id: &issue.id,
			run_id: "run-repair",
			attempt_number: 2,
			phase: "repair",
			review_level: config.codex().review_level().as_str(),
			status: "clean",
			head_sha: repaired_head_oid,
			nonclean_rounds: 0,
			details_json: "{}",
		})
		.expect("clean repair checkpoint should record");
	state_store
		.clear_review_policy_checkpoints_for_run_attempt(
			config.service_id(),
			&issue.id,
			"run-repair",
			2,
		)
		.expect("active repair checkpoint row should clear after completion");
	state_store
		.append_private_execution_event(
			config.service_id(),
			&issue.id,
			"run-repair",
			2,
			"review_completion_intent",
			serde_json::json!({
				"path": "review_repair",
				"mode": "repair",
				"branch": &worktree.branch_name,
				"worktree_path": worktree.path.display().to_string(),
				"pr_url": pr_url,
				"pr_base_ref": "main",
				"pr_head_ref": &worktree.branch_name,
				"pr_head_oid": repaired_head_oid,
				"summary": "Review repair is clean."
			}),
		)
		.expect("repair completion intent should record");
	state_store
		.append_private_execution_event(
			config.service_id(),
			&issue.id,
			"run-repair",
			2,
			"terminal_finalize",
			serde_json::json!({
				"path": "review_repair",
				"mode": "repair",
				"branch": &worktree.branch_name,
				"worktree_path": worktree.path.display().to_string(),
			}),
		)
		.expect("repair terminal finalize should record");
}
