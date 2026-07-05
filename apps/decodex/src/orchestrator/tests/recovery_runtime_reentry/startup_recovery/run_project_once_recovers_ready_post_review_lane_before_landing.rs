use std::fs;

use crate::{
	orchestrator::{
		self, ReviewLevel,
		tests::{self, FakeTracker, recovery_terminal_support},
	},
	state::StateStore,
	worktree::WorktreeManager,
};

#[test]
fn run_project_once_recovers_ready_post_review_lane_before_landing() {
	let (temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_review_level(
		&tests::service_config_with_github_token_env_var(&base_config, "PATH"),
		ReviewLevel::Standard,
	);
	let issue = recovery_terminal_support::sample_active_issue("In Review");
	let tracker = FakeTracker::with_refresh_snapshots(
		vec![issue.clone()],
		vec![vec![issue.clone()], vec![issue.clone()], vec![issue.clone()]],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("retained review worktree should be created");
	let pr_url = "https://github.com/hack-ink/decodex/pull/333";
	let head_subject =
		r#"{"schema":"decodex/commit/1","summary":"Add retry hint","authority":"PUB-101"}"#;
	let landed_subject =
		r#"{"schema":"decodex/commit/1","summary":"Land Add retry hint","authority":"PUB-101"}"#;
	let head_oid = tests::commit_worktree_change(
		&worktree.path,
		"retained-ready.txt",
		"ready\n",
		head_subject,
	);
	let (_path_guard, invocation_log_path) =
		recovery_terminal_support::install_fake_ready_to_land_admin_merge_gh_response(
			&temp_dir, &worktree, pr_url, &head_oid,
		);

	tests::seed_review_handoff_marker_value(
		&state_store,
		config.service_id(),
		&issue.id,
		&tests::sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
	);

	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, false)
		.expect("recovered retained post-review lane should reconcile");

	assert!(
		summary.is_none(),
		"ready retained post-review landing should not dispatch a new current lane"
	);

	let marker = tests::persisted_review_orchestration_marker_for_path(
		&state_store,
		config.service_id(),
		&worktree.path,
	);
	let gh_invocation = fs::read_to_string(&invocation_log_path)
		.expect("fake gh invocation log should read")
		.lines()
		.map(str::to_owned)
		.collect::<Vec<_>>();

	assert_eq!(marker.phase(), "waiting_for_merge");
	assert_eq!(
		gh_invocation,
		vec![
			String::from("pr"),
			String::from("merge"),
			String::from("--admin"),
			String::from("--merge"),
			String::from("--match-head-commit"),
			head_oid,
			String::from("--subject"),
			String::from(landed_subject),
			String::from("--body"),
			String::new(),
			String::from(pr_url),
		]
	);
}
