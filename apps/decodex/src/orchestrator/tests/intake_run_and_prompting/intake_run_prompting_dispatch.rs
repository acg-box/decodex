use std::path::{Path, PathBuf};

use crate::{
	orchestrator::{
		self, IssueDispatchMode, RunSummary, TargetIssueRunContext,
		tests::{
			self, FakeTracker, TEST_SERVICE_ID, intake_run_and_prompting, recovery_terminal_support,
		},
	},
	state::{self, StateStore},
	tracker,
	worktree::WorktreeManager,
};

#[test]
fn dry_run_selects_one_issue_and_plans_worktree() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let tracker = FakeTracker::new(vec![tests::sample_issue("Todo", &[])]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, true)
		.expect("run once should succeed")
		.expect("one issue should be selected");

	assert_eq!(
		summary,
		RunSummary {
			project_id: String::from("pubfi"),
			issue_id: String::from("issue-1"),
			issue_identifier: String::from("PUB-101"),
			issue_state: String::from("In Progress"),
			initial_issue_state: String::from("Todo"),
			retry_project_slug: String::new(),
			dispatch_mode: IssueDispatchMode::Normal,
			branch_name: String::from("x/pubfi-pub-101"),
			worktree_path: Path::new(&config.worktree_root().join("PUB-101")).to_path_buf(),
			attempt_number: 1,
			run_id: summary.run_id.clone(),
			continuation_pending: false,
		}
	);
	assert!(tracker.comments.borrow().is_empty());
}

#[test]
fn targeted_identifier_dispatch_accepts_status_ready_queued_issue() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue_with_sort_fields(
		"issue-ready",
		"PUB-101",
		"Todo",
		&[],
		Some(1),
		"2026-03-13T04:16:17.133Z",
	);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("status snapshot should build");
	let candidate = snapshot
		.queued_candidates
		.iter()
		.find(|candidate| candidate.issue_identifier == issue.identifier)
		.expect("queued issue should appear in status");

	assert_eq!(candidate.classification, "ready");
	assert_eq!(candidate.reason, "eligible_for_dispatch");

	let summary =
		orchestrator::run_target_issue_once_with_inferred_dispatch(TargetIssueRunContext {
			tracker: &tracker,
			project: &config,
			workflow: &workflow,
			state_store: &state_store,
			issue_id: &issue.identifier,
			preferred_issue_state: None,
			preferred_initial_issue_state: None,
			dry_run: true,
			lease_preacquired: false,
			preferred_issue_claim_fd: None,
			preferred_dispatch_slot_fd: None,
			preferred_dispatch_slot_index: None,
			dispatch_mode: IssueDispatchMode::Normal,
			preferred_run_identity: None,
			preferred_retry_budget_base: None,
		})
		.expect("targeted identifier run should succeed")
		.expect("status-ready queued issue should dispatch by identifier");

	assert_eq!(summary.issue_id, issue.id);
	assert_eq!(summary.issue_identifier, issue.identifier);
	assert_eq!(summary.dispatch_mode, IssueDispatchMode::Normal);
}

#[test]
fn targeted_inferred_dispatch_keeps_retry_for_active_issue() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = intake_run_and_prompting::run_and_prompting_service_owned_issue("In Progress");
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let summary =
		orchestrator::run_target_issue_once_with_inferred_dispatch(TargetIssueRunContext {
			tracker: &tracker,
			project: &config,
			workflow: &workflow,
			state_store: &state_store,
			issue_id: &issue.identifier,
			preferred_issue_state: None,
			preferred_initial_issue_state: None,
			dry_run: true,
			lease_preacquired: false,
			preferred_issue_claim_fd: None,
			preferred_dispatch_slot_fd: None,
			preferred_dispatch_slot_index: None,
			dispatch_mode: IssueDispatchMode::Normal,
			preferred_run_identity: None,
			preferred_retry_budget_base: None,
		})
		.expect("targeted active identifier run should succeed")
		.expect("active target should fall back to retry dispatch");

	assert_eq!(summary.issue_id, issue.id);
	assert_eq!(summary.issue_identifier, issue.identifier);
	assert_eq!(summary.dispatch_mode, IssueDispatchMode::Retry);
}

#[test]
fn targeted_identifier_dispatch_accepts_status_visible_retained_closeout_lane() {
	let (temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(&base_config, "HOME");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = intake_run_and_prompting::run_and_prompting_service_owned_issue("In Review");
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree =
		worktree_manager.ensure_worktree(&issue.identifier, false).expect("worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/181";

	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree should record");

	tests::seed_review_handoff_marker_for_path(
		&state_store,
		config.service_id(),
		&worktree.path,
		&tests::sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
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

	assert_eq!(lane.classification, "continue");
	assert_eq!(lane.reason, "pull_request_merged_closeout_pending");
	assert_eq!(lane.pr_state.as_deref(), Some("MERGED"));

	let summary =
		orchestrator::run_target_issue_once_with_inferred_dispatch(TargetIssueRunContext {
			tracker: &tracker,
			project: &config,
			workflow: &workflow,
			state_store: &state_store,
			issue_id: &issue.identifier,
			preferred_issue_state: None,
			preferred_initial_issue_state: None,
			dry_run: true,
			lease_preacquired: false,
			preferred_issue_claim_fd: None,
			preferred_dispatch_slot_fd: None,
			preferred_dispatch_slot_index: None,
			dispatch_mode: IssueDispatchMode::Normal,
			preferred_run_identity: None,
			preferred_retry_budget_base: None,
		})
		.expect("targeted retained closeout identifier run should succeed")
		.expect("status-visible retained closeout lane should dispatch by identifier");

	assert_eq!(summary.issue_id, issue.id);
	assert_eq!(summary.issue_identifier, issue.identifier);
	assert_eq!(summary.dispatch_mode, IssueDispatchMode::Closeout);
	assert_eq!(summary.run_id, "run-1");
	assert_eq!(summary.attempt_number, 1);
}

#[test]
fn targeted_identifier_dispatch_accepts_status_visible_review_repair_lane() {
	let (temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(&base_config, "HOME");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = intake_run_and_prompting::run_and_prompting_service_owned_issue("In Review");
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree =
		worktree_manager.ensure_worktree(&issue.identifier, false).expect("worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/184";

	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree should record");

	tests::seed_review_handoff_marker_for_path(
		&state_store,
		config.service_id(),
		&worktree.path,
		&tests::sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
	);

	let _path_guard = recovery_terminal_support::install_fake_conflicting_pr_gh_response(
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
		.expect("retained repair lane should appear in status");

	assert_eq!(lane.classification, "needs_review_repair");
	assert_eq!(lane.reason, "pull_request_merge_conflict");

	let summary =
		orchestrator::run_target_issue_once_with_inferred_dispatch(TargetIssueRunContext {
			tracker: &tracker,
			project: &config,
			workflow: &workflow,
			state_store: &state_store,
			issue_id: &issue.identifier,
			preferred_issue_state: None,
			preferred_initial_issue_state: None,
			dry_run: true,
			lease_preacquired: false,
			preferred_issue_claim_fd: None,
			preferred_dispatch_slot_fd: None,
			preferred_dispatch_slot_index: None,
			dispatch_mode: IssueDispatchMode::Normal,
			preferred_run_identity: None,
			preferred_retry_budget_base: None,
		})
		.expect("targeted retained repair identifier run should succeed")
		.expect("status-visible retained repair lane should dispatch by identifier");

	assert_eq!(summary.issue_id, issue.id);
	assert_eq!(summary.issue_identifier, issue.identifier);
	assert_eq!(summary.dispatch_mode, IssueDispatchMode::ReviewRepair);
	assert_eq!(summary.issue_state, "In Review");
}

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
	tests::seed_review_handoff_marker_for_path(
		&state_store,
		config.service_id(),
		&worktree.path,
		&tests::sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
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

#[test]
fn targeted_identifier_dispatch_rejects_different_status_visible_closeout_lane() {
	let (temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(&base_config, "HOME");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let closeout_issue = tests::sample_issue_with_sort_fields(
		"issue-closeout",
		"PUB-101",
		"In Review",
		&[active_label.as_str()],
		Some(1),
		"2026-03-13T04:16:17.133Z",
	);
	let requested_issue = tests::sample_issue_with_sort_fields(
		"issue-requested",
		"PUB-102",
		"In Review",
		&[active_label.as_str()],
		Some(2),
		"2026-03-13T04:17:17.133Z",
	);
	let tracker = FakeTracker::new(vec![closeout_issue.clone(), requested_issue.clone()]);
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&closeout_issue.identifier, false)
		.expect("worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/182";

	state_store
		.upsert_worktree(
			config.service_id(),
			&closeout_issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree should record");

	tests::seed_review_handoff_marker_for_path(
		&state_store,
		config.service_id(),
		&worktree.path,
		&tests::sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
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

	assert_eq!(snapshot.post_review_lanes.len(), 1);
	assert_eq!(snapshot.post_review_lanes[0].issue_identifier, closeout_issue.identifier);
	assert_eq!(snapshot.post_review_lanes[0].classification, "continue");
	assert_eq!(snapshot.post_review_lanes[0].reason, "pull_request_merged_closeout_pending",);

	let error = orchestrator::run_target_issue_once_with_inferred_dispatch(TargetIssueRunContext {
		tracker: &tracker,
		project: &config,
		workflow: &workflow,
		state_store: &state_store,
		issue_id: &requested_issue.identifier,
		preferred_issue_state: None,
		preferred_initial_issue_state: None,
		dry_run: true,
		lease_preacquired: false,
		preferred_issue_claim_fd: None,
		preferred_dispatch_slot_fd: None,
		preferred_dispatch_slot_index: None,
		dispatch_mode: IssueDispatchMode::Normal,
		preferred_run_identity: None,
		preferred_retry_budget_base: None,
	})
	.expect_err("targeted closeout inference should reject a different visible lane");
	let message = error.to_string();

	assert!(message.contains("targeted retained closeout mismatch"));
	assert!(message.contains(&requested_issue.identifier));
	assert!(message.contains(&closeout_issue.identifier));
}

#[test]
fn targeted_identifier_dispatch_rejects_different_status_visible_review_repair_lane() {
	let (temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(&base_config, "HOME");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let repair_issue = tests::sample_issue_with_sort_fields(
		"issue-repair",
		"PUB-201",
		"In Review",
		&[active_label.as_str()],
		Some(1),
		"2026-03-13T04:16:17.133Z",
	);
	let requested_issue = tests::sample_issue_with_sort_fields(
		"issue-requested",
		"PUB-202",
		"In Review",
		&[active_label.as_str()],
		Some(2),
		"2026-03-13T04:17:17.133Z",
	);
	let tracker = FakeTracker::new(vec![repair_issue.clone(), requested_issue.clone()]);
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&repair_issue.identifier, false)
		.expect("worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/185";

	state_store
		.upsert_worktree(
			config.service_id(),
			&repair_issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree should record");

	tests::seed_review_handoff_marker_for_path(
		&state_store,
		config.service_id(),
		&worktree.path,
		&tests::sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
	);

	let _path_guard = recovery_terminal_support::install_fake_conflicting_pr_gh_response(
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

	assert_eq!(snapshot.post_review_lanes.len(), 1);
	assert_eq!(snapshot.post_review_lanes[0].issue_identifier, repair_issue.identifier);
	assert_eq!(snapshot.post_review_lanes[0].classification, "needs_review_repair");
	assert_eq!(snapshot.post_review_lanes[0].reason, "pull_request_merge_conflict");

	let error = orchestrator::run_target_issue_once_with_inferred_dispatch(TargetIssueRunContext {
		tracker: &tracker,
		project: &config,
		workflow: &workflow,
		state_store: &state_store,
		issue_id: &requested_issue.identifier,
		preferred_issue_state: None,
		preferred_initial_issue_state: None,
		dry_run: true,
		lease_preacquired: false,
		preferred_issue_claim_fd: None,
		preferred_dispatch_slot_fd: None,
		preferred_dispatch_slot_index: None,
		dispatch_mode: IssueDispatchMode::Normal,
		preferred_run_identity: None,
		preferred_retry_budget_base: None,
	})
	.expect_err("targeted review repair inference should reject a different visible lane");
	let message = error.to_string();

	assert!(message.contains("targeted retained review repair mismatch"));
	assert!(message.contains(&requested_issue.identifier));
	assert!(message.contains(&repair_issue.identifier));
}

#[test]
fn format_run_once_summary_surfaces_continuation_boundaries() {
	let summary = RunSummary {
		project_id: String::from("pubfi"),
		issue_id: String::from("issue-1"),
		issue_identifier: String::from("PUB-101"),
		issue_state: String::from("In Progress"),
		initial_issue_state: String::from("In Progress"),
		retry_project_slug: String::from("pubfi"),
		dispatch_mode: IssueDispatchMode::Normal,
		branch_name: String::from("x/pubfi-pub-101"),
		worktree_path: PathBuf::from(".worktrees/PUB-101"),
		attempt_number: 1,
		run_id: String::from("pub-101-attempt-1"),
		continuation_pending: true,
	};
	let message = orchestrator::format_run_once_summary(&summary, false);

	assert!(message.contains("run paused at continuation boundary"));
	assert!(message.contains("next_action=rerun_or_use_daemon"));
	assert!(!message.contains("run complete"));
}

#[test]
fn dry_run_returns_none_when_intake_has_no_service_owned_candidate() {
	{
		let (_temp_dir, config, workflow) = tests::temp_project_layout();
		let tracker = FakeTracker::with_refresh_snapshots_and_project(vec![], vec![vec![]], false);
		let state_store = StateStore::open_in_memory().expect("state store should open");
		let summary =
			orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, true)
				.expect("dry run without queued issues should succeed");

		assert!(summary.is_none(), "empty intake should simply produce no dry-run selection");
	}
	{
		let (_temp_dir, config, workflow) = tests::temp_project_layout();
		let issue = tests::sample_issue_with_project_slug_and_sort_fields(
			"issue-1",
			"PUB-101",
			"other-service",
			"Todo",
			&[],
			Some(3),
			"2026-03-13T04:16:17.133Z",
		);
		let tracker = FakeTracker::new(vec![issue]);
		let state_store = StateStore::open_in_memory().expect("state store should open");
		let summary =
			orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, true)
				.expect("dry run should succeed");

		assert!(summary.is_none(), "service-scoped queue labels should isolate intake");
	}
}

#[test]
fn no_eligible_issue_message_includes_operator_hint() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let message = orchestrator::format_no_eligible_issue_message(&config, &workflow);

	assert!(message.contains("No eligible issue found for the configured project."));
	assert!(message.contains("`Todo`"));
	assert!(message.contains("`decodex:queued:<service-id>`"));
	assert!(message.contains("`decodex:queued:pubfi`"));
	assert!(message.contains("`decodex:manual-only`/`decodex:needs-attention`"));
	assert!(message.contains("non-terminal state"));
	assert!(message.contains("dependency blockers"));
	assert!(message.contains("no active issue claim"));
}

#[test]
fn dry_run_falls_back_to_normal_issue_when_retained_retry_loses_ownership() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let normal_issue = tests::sample_issue_with_sort_fields(
		"issue-2",
		"PUB-102",
		"Todo",
		&[],
		Some(1),
		"2026-03-13T04:17:17.133Z",
	);
	let retry_issue =
		intake_run_and_prompting::run_and_prompting_service_owned_issue("In Progress");
	let retry_issue_without_ownership = tests::sample_issue("In Progress", &[]);
	let tracker = FakeTracker::with_refresh_snapshots(
		vec![normal_issue.clone(), retry_issue.clone()],
		vec![
			vec![retry_issue.clone()],
			vec![retry_issue.clone()],
			vec![retry_issue_without_ownership.clone()],
			vec![retry_issue_without_ownership],
			vec![tests::sample_issue("In Progress", &[])],
			vec![normal_issue.clone()],
		],
	);
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&retry_issue.identifier, false)
		.expect("retained retry worktree should exist");

	state_store
		.upsert_worktree(
			config.service_id(),
			&retry_issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree mapping should record");

	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, true)
		.expect("dry run should succeed")
		.expect("normal queued issue should be selected after retained retry is excluded");

	assert_eq!(summary.issue_identifier, normal_issue.identifier);
	assert_eq!(summary.dispatch_mode, IssueDispatchMode::Normal);
}
