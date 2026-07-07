use crate::{
	orchestrator::{
		self, IssueDispatchMode,
		tests::{self, FakeTracker, intake_run_and_prompting, recovery_terminal_support},
	},
	state::{self, StateStore},
	worktree::WorktreeManager,
};

#[test]
fn targeted_identifier_dispatch_accepts_stopped_active_closeout_lease() {
	let (temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(&base_config, "HOME");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = intake_run_and_prompting::run_and_prompting_service_owned_issue("Done");
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree =
		worktree_manager.ensure_worktree(&issue.identifier, false).expect("worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/183";

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "running")
		.expect("stopped running run attempt should record");
	state_store
		.upsert_lease(config.service_id(), &issue.id, "run-1", "Done")
		.expect("stopped closeout lease should record");
	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree should record");

	state::write_run_activity_marker_for_process(&worktree.path, "run-1", 1, u32::MAX)
		.expect("stopped closeout activity marker should write");
	tests::seed_review_lifecycle_handoff_fixture_for_path(
		&state_store,
		config.service_id(),
		&worktree.path,
		&tests::sample_review_lifecycle_handoff_fixture(&worktree.branch_name, pr_url, &head_oid),
	);

	let _path_guard = recovery_terminal_support::install_fake_merged_pr_gh_response(
		&temp_dir, &worktree, pr_url, &head_oid,
	);
	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("status snapshot should build");
	let lane = snapshot
		.post_review_lanes
		.iter()
		.find(|lane| lane.issue_identifier == issue.identifier)
		.expect("retained closeout lane should appear in status");
	let target_context = |dispatch_mode| {
		intake_run_and_prompting::run_and_prompting_target_context(
			&tracker,
			&config,
			&workflow,
			&state_store,
			&issue.identifier,
			dispatch_mode,
		)
	};

	assert_eq!(lane.classification, "continue");
	assert_eq!(lane.reason, "pull_request_merged_closeout_pending");
	assert_eq!(lane.pr_state.as_deref(), Some("MERGED"));
	assert_eq!(snapshot.current_lanes.len(), 1);
	assert_eq!(snapshot.current_lanes[0].process_alive, Some(false));
	assert!(
		orchestrator::issue_passes_closeout_dispatch_policy(
			&tracker,
			&issue,
			&config,
			&workflow,
			&state_store,
		)
		.expect("closeout dispatch policy should evaluate"),
		"stopped active closeout lease should still satisfy closeout policy"
	);
	assert!(
		!orchestrator::target_issue_active_claim_blocks_dispatch(
			&target_context(IssueDispatchMode::Closeout),
			&issue.id,
			&issue,
		)
		.expect("active closeout claim guard should evaluate"),
		"stopped active closeout lease should not block closeout dispatch"
	);

	let explicit_summary =
		orchestrator::run_target_issue_once(target_context(IssueDispatchMode::Closeout))
			.expect("explicit retained closeout identifier run should succeed")
			.expect("explicit closeout should accept the stopped active closeout lease");

	assert_eq!(explicit_summary.dispatch_mode, IssueDispatchMode::Closeout);

	let summary = orchestrator::run_target_issue_once_with_inferred_dispatch(target_context(
		IssueDispatchMode::Normal,
	))
	.expect("targeted retained closeout identifier run should succeed")
	.expect("stopped active closeout lease should not hide the closeout candidate");

	assert_eq!(summary.issue_id, issue.id);
	assert_eq!(summary.issue_identifier, issue.identifier);
	assert_eq!(summary.dispatch_mode, IssueDispatchMode::Closeout);
	assert_eq!(summary.run_id, "run-1");
	assert_eq!(summary.attempt_number, 1);
}
