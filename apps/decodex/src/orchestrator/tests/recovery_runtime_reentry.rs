use std::{
	fs,
	process::{self, Command},
};

use time::OffsetDateTime;

use crate::{
	agent::RUN_LEASE_IDLE_TIMEOUT,
	orchestrator::{
		self, RecoverableWorktreeSkipCache, ReviewLevel, RunLeaseDisposition,
		TERMINAL_GUARD_MARKER_FILE,
		tests::{
			FakeTracker, TEST_SERVICE_ID, recovery_terminal_support, {self},
		},
	},
	state::{self, ProtocolActivityMarker, StateStore},
	tracker::{self},
	worktree::WorktreeManager,
};

#[test]
fn exited_child_reconciliation_detects_stalled_failed_runs_from_protocol_idle() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue_with_sort_fields(
		"issue-stalled-after-exit",
		"PUB-205",
		"In Progress",
		&[],
		Some(3),
		"2026-03-13T04:16:17.133Z",
	);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let run_id = "run-stalled-after-exit";
	let worktree_path = config.worktree_root().join(&issue.identifier);

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-205",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");
	state_store
		.record_run_attempt(run_id, &issue.id, 1, "failed")
		.expect("run should exit as failed before daemon inspects it");

	state::write_run_protocol_activity_marker(
		&worktree_path,
		&ProtocolActivityMarker {
			run_id,
			attempt_number: 1,
			thread_id: None,
			turn_id: None,
			event_count: 1,
			last_event_type: "thread/status/changed",
			child_agent_activity: None,
			protocol_activity: None,
		},
	)
	.expect("protocol marker should write");

	let last_protocol_activity =
		state::read_run_protocol_activity_marker(&worktree_path, run_id, 1)
			.expect("protocol marker should read")
			.expect("protocol activity should exist");
	let actions = orchestrator::inspect_exited_daemon_child_reconciliation_at(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&issue.id,
		run_id,
		last_protocol_activity + RUN_LEASE_IDLE_TIMEOUT.as_secs() as i64 + 1,
	)
	.expect("exited child inspection should succeed");

	assert!(actions.iter().any(|action| {
		action.issue.id == issue.id
			&& matches!(
				action.disposition,
				RunLeaseDisposition::Stalled{ idle_for }
					if idle_for >= RUN_LEASE_IDLE_TIMEOUT
			)
	}));
}

#[test]
fn exited_child_reconciliation_detects_retained_partial_progress_from_dirty_worktree() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue_with_sort_fields(
		"issue-stalled-dirty-after-exit",
		"PUB-206",
		"In Progress",
		&[],
		Some(3),
		"2026-03-13T04:16:17.133Z",
	);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let run_id = "run-stalled-dirty-after-exit";
	let worktree_path = config.worktree_root().join(&issue.identifier);

	tests::git_status_success(
		config.repo_root(),
		&["worktree", "add", "-b", "x/pubfi-pub-206", ".worktrees/PUB-206", "main"],
	);
	fs::write(worktree_path.join("README.md"), "retained partial work\n")
		.expect("tracked worktree file should change");

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "failed")
		.expect("run should exit as failed before daemon inspects it");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-206",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");

	state::write_run_protocol_activity_marker(
		&worktree_path,
		&ProtocolActivityMarker {
			run_id,
			attempt_number: 1,
			thread_id: None,
			turn_id: None,
			event_count: 1,
			last_event_type: "turn/diff/updated",
			child_agent_activity: None,
			protocol_activity: None,
		},
	)
	.expect("protocol marker should write");

	let last_protocol_activity =
		state::read_run_protocol_activity_marker(&worktree_path, run_id, 1)
			.expect("protocol marker should read")
			.expect("protocol activity should exist");
	let actions = orchestrator::inspect_exited_daemon_child_reconciliation_at(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&issue.id,
		run_id,
		last_protocol_activity + RUN_LEASE_IDLE_TIMEOUT.as_secs() as i64 + 1,
	)
	.expect("exited child inspection should succeed");

	assert_eq!(actions.len(), 1);
	assert!(matches!(
		actions[0].disposition,
		RunLeaseDisposition::StalledRetainedPartialProgress { idle_for }
			if idle_for >= RUN_LEASE_IDLE_TIMEOUT
	));
}

#[test]
fn exited_child_reconciliation_ignores_superseded_failed_run() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue_with_sort_fields(
		"issue-superseded-after-exit",
		"PUB-206",
		"In Progress",
		&[],
		Some(3),
		"2026-03-13T04:16:17.133Z",
	);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let stale_run_id = "run-superseded-after-exit-1";
	let newer_run_id = "run-superseded-after-exit-2";
	let worktree_path = config.worktree_root().join(&issue.identifier);
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");

	state_store
		.record_run_attempt(stale_run_id, &issue.id, 1, "failed")
		.expect("stale run should record");
	state_store
		.record_run_attempt(newer_run_id, &issue.id, 2, "running")
		.expect("newer run should record");
	state_store
		.upsert_lease(config.service_id(), &issue.id, newer_run_id, "In Progress")
		.expect("newer lease should record");
	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			"x/pubfi-pub-206",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");

	state::write_run_protocol_activity_marker(
		&worktree_path,
		&ProtocolActivityMarker {
			run_id: stale_run_id,
			attempt_number: 1,
			thread_id: None,
			turn_id: None,
			event_count: 1,
			last_event_type: "thread/status/changed",
			child_agent_activity: None,
			protocol_activity: None,
		},
	)
	.expect("protocol marker should write");

	let actions = orchestrator::inspect_exited_daemon_child_reconciliation_at(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&issue.id,
		stale_run_id,
		OffsetDateTime::now_utc().unix_timestamp() + RUN_LEASE_IDLE_TIMEOUT.as_secs() as i64 + 1,
	)
	.expect("exited child inspection should succeed");

	assert_eq!(actions.len(), 1);
	assert!(matches!(
		&actions[0].disposition,
		RunLeaseDisposition::Superseded {
			newer_run_id: observed_run_id,
			newer_attempt_number: 2,
		} if observed_run_id == newer_run_id
	));

	orchestrator::apply_run_lease_reconciliation(
		&tracker,
		&config,
		&state_store,
		&worktree_manager,
		actions,
	)
	.expect("superseded reconciliation should succeed");

	let lease = state_store
		.lease_for_issue(&issue.id)
		.expect("lease lookup should succeed")
		.expect("newer lease should remain");

	assert_eq!(lease.run_id(), newer_run_id);
	assert!(
		tracker.comments.borrow().is_empty(),
		"superseded stale child must not write needs-attention comments"
	);
}

#[test]
fn run_project_once_prefers_recovered_in_progress_worktree_after_empty_state_startup() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = recovery_terminal_support::sample_active_issue("In Progress");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let expected_worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("recovered worktree should be created")
		.path;
	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, true)
		.expect("recovered dry run should succeed")
		.expect("active recovered issue should be selected");

	assert_eq!(summary.issue_id, issue.id);
	assert_eq!(summary.dispatch_mode, orchestrator::IssueDispatchMode::Retry);
	assert_eq!(summary.worktree_path, expected_worktree);
	assert!(
		state_store
			.worktree_for_issue(&issue.id)
			.expect("worktree lookup should succeed")
			.is_some(),
		"worktree mapping should be reconstructed from the retained lane"
	);
}

#[test]
fn recover_runtime_state_recovers_fresh_review_repair_activity_marker() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = recovery_terminal_support::sample_active_issue("In Review");
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("review-repair worktree should exist");

	state::write_run_activity_marker(&worktree.path, "run-review-repair", 1)
		.expect("fresh activity marker should write");

	let recovered = orchestrator::recover_runtime_state_from_tracker_and_worktrees(
		&tracker,
		&config,
		&workflow,
		&state_store,
	)
	.expect("runtime recovery should succeed");

	assert!(
		recovered.recoverable_issues.is_empty(),
		"fresh review-repair activity should rebuild the lease instead of requeueing the lane"
	);

	let lease = state_store
		.lease_for_issue(&issue.id)
		.expect("lease lookup should succeed")
		.expect("fresh review-repair lane should rebuild its lease");

	assert_eq!(lease.run_id(), "run-review-repair");
	assert_eq!(lease.issue_state(), workflow.frontmatter().tracker().success_state());
}

#[test]
fn run_project_once_recovers_retained_worktree_from_issue_identifier() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let issue = tests::sample_issue_with_project_slug_and_sort_fields(
		"issue-1",
		"PUB-101",
		"tracker-project",
		"In Progress",
		&[active_label.as_str()],
		Some(3),
		"2026-03-13T04:16:17.133Z",
	);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let expected_worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("recovered worktree should be created")
		.path;
	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, true)
		.expect("recovered dry run should succeed")
		.expect("active recovered issue should be selected");

	assert_eq!(summary.issue_id, issue.id);
	assert_eq!(summary.dispatch_mode, orchestrator::IssueDispatchMode::Retry);
	assert_eq!(summary.worktree_path, expected_worktree);
}

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

#[test]
fn materialize_run_summary_worktree_creates_worktree_before_child_activity_marker() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("Todo", &[]);
	let tracker = FakeTracker::with_refresh_snapshots(
		vec![issue.clone()],
		vec![vec![issue.clone()], vec![issue.clone()]],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, true)
		.expect("dry-run planning should succeed")
		.expect("brand-new lane should be selected");

	assert!(
		!summary.worktree_path.exists(),
		"dry-run planning should not materialize the worktree yet"
	);

	let worktree = orchestrator::materialize_run_summary_worktree(&config, &workflow, &summary)
		.expect("daemon parent should materialize the worktree before child startup");

	assert_eq!(worktree.path, summary.worktree_path);
	assert_eq!(worktree.branch_name, summary.branch_name);
	assert!(
		worktree.path.exists(),
		"materialized worktree should exist before writing child activity markers"
	);

	state::write_run_activity_marker_for_process(
		&worktree.path,
		&summary.run_id,
		summary.attempt_number,
		process::id(),
	)
	.expect("child activity marker should write after worktree materialization");
}

#[test]
fn cleanup_terminal_worktree_runs_before_remove_workspace_hook() {
	let workflow_markdown = r#"
+++
version = 1

[tracker]
provider = "linear"
startable_states = ["Todo"]
terminal_states = ["Done", "Canceled", "Duplicate"]
in_progress_state = "In Progress"
success_state = "In Review"
completed_state = "Done"
failure_state = "Todo"
opt_out_label = "decodex:manual-only"
needs_attention_label = "decodex:needs-attention"

[agent]
transport = "stdio://"

[execution]
max_attempts = 3
max_turns = 1
max_retry_backoff_ms = 300000
gate_profiles = {}
canonicalize_commands = []
verify_commands = []

[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = ["printf '%s:%s\n' \"$DECODEX_ISSUE_ID\" \"$DECODEX_BRANCH\" > \"$DECODEX_REPO_ROOT/before-remove.log\""]
timeout_seconds = 60

[context]
read_first = []
+++

Follow the repository policy.
	"#;
	let (_temp_dir, config, workflow) =
		tests::temp_project_layout_with_workflow_markdown(workflow_markdown);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree("PUB-101", false)
		.expect("worktree should exist before cleanup");

	orchestrator::cleanup_terminal_worktree(
		&state_store,
		&worktree_manager,
		&workflow,
		"issue-1",
		"PUB-101",
		&worktree.branch_name,
		&worktree.path,
	)
	.expect("cleanup should succeed");

	assert_eq!(
		fs::read_to_string(config.repo_root().join("before-remove.log"))
			.expect("before-remove hook log should exist"),
		"PUB-101:x/pubfi-pub-101\n"
	);
	assert!(!worktree.path.exists(), "cleanup should still remove the worktree");
	assert!(
		!tests::git_output(config.repo_root(), &["branch", "--list", &worktree.branch_name])
			.is_empty(),
		"generic terminal cleanup should preserve the retained local branch ref"
	);
}

#[test]
fn materialize_daemon_spawn_state_starts_fresh_budget_for_normal_queue_intake() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("Todo", &[]);
	let tracker = FakeTracker::with_refresh_snapshots(
		vec![issue.clone()],
		vec![vec![issue.clone()], vec![issue.clone()]],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let retained_worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("retained worktree should exist");

	state::write_run_retry_budget_attempt_count(&retained_worktree.path, "older-run", 4, 2)
		.expect("retry budget marker should write");

	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, true)
		.expect("dry-run planning should succeed")
		.expect("retained lane should still be selected");
	let daemon_spawn_state =
		orchestrator::materialize_daemon_spawn_state(&config, &workflow, &state_store, &summary)
			.expect("daemon parent should materialize worktree and retry budget together");

	assert_eq!(daemon_spawn_state.worktree.path, summary.worktree_path);
	assert_eq!(
		daemon_spawn_state.retry_budget_base, 0,
		"normal daemon queue intake should not inherit retry attempts from an old marker"
	);
}

#[test]
fn materialize_daemon_spawn_state_uses_retained_retry_budget_marker_for_recovered_retry() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = recovery_terminal_support::sample_active_issue("In Progress");
	let tracker = FakeTracker::with_refresh_snapshots(
		vec![issue.clone()],
		vec![vec![issue.clone()], vec![issue.clone()]],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let retained_worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("retained worktree should exist");

	state::write_run_retry_budget_attempt_count(&retained_worktree.path, "older-run", 4, 2)
		.expect("retry budget marker should write");

	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, true)
		.expect("dry-run planning should succeed")
		.expect("retained lane should still be selected");
	let daemon_spawn_state =
		orchestrator::materialize_daemon_spawn_state(&config, &workflow, &state_store, &summary)
			.expect("daemon parent should materialize worktree and retry budget together");

	assert_eq!(summary.dispatch_mode, orchestrator::IssueDispatchMode::Retry);
	assert_eq!(daemon_spawn_state.worktree.path, summary.worktree_path);
	assert_eq!(
		daemon_spawn_state.retry_budget_base, 2,
		"recovered retry handoff should preserve retry budget from the retained worktree marker"
	);
}

#[test]
fn run_project_once_skips_recovered_worktree_with_fresh_activity_marker() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = recovery_terminal_support::sample_active_issue("In Progress");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("recovered worktree should exist");

	state::write_run_activity_marker(&worktree.path, "run-1", 1)
		.expect("activity marker should write");

	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, true)
		.expect("recovery should succeed");

	assert!(
		summary.is_none(),
		"fresh child activity should recover as a current lane instead of redispatching"
	);
	assert_eq!(
		state_store
			.lease_for_issue(&issue.id)
			.expect("lease lookup should succeed")
			.expect("lease should be reconstructed")
			.run_id(),
		"run-1"
	);
	assert_eq!(
		state_store
			.run_attempt("run-1")
			.expect("run attempt lookup should succeed")
			.expect("run attempt should be reconstructed")
			.status(),
		"running"
	);
}

#[cfg(unix)]
#[test]
fn run_project_once_retries_recovered_worktree_after_marker_process_is_killed() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = recovery_terminal_support::sample_active_issue("In Progress");
	let tracker = FakeTracker::with_refresh_snapshots(
		vec![issue.clone()],
		vec![vec![issue.clone()], vec![issue.clone()]],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("recovered worktree should exist");
	let mut child = Command::new("/bin/sh")
		.args(["-c", "exec sleep 60"])
		.spawn()
		.expect("kill-smoke child process should start");
	let child_process_id = child.id();

	assert!(
		orchestrator::process_is_alive(child_process_id),
		"kill-smoke child process should be live before marker write"
	);

	state::write_run_activity_marker_for_process(&worktree.path, "run-1", 1, child_process_id)
		.expect("activity marker should write");

	child.kill().expect("kill-smoke child process should be killed");
	child.wait().expect("kill-smoke child process should be reaped");

	assert!(
		!orchestrator::process_is_alive(child_process_id),
		"kill-smoke child process should no longer be live after kill"
	);

	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, true)
		.expect("kill-smoke recovery should succeed")
		.expect("killed-process recovered lane should be selected for retry");

	assert_eq!(summary.issue_id, issue.id);
	assert_eq!(summary.dispatch_mode, orchestrator::IssueDispatchMode::Retry);
	assert!(
		state_store.lease_for_issue(&issue.id).expect("lease lookup should succeed").is_none(),
		"killed marker processes must not reconstruct live leases"
	);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn run_project_once_retries_recovered_worktree_from_previous_boot() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = recovery_terminal_support::sample_active_issue("In Progress");
	let tracker = FakeTracker::with_refresh_snapshots(
		vec![issue.clone()],
		vec![vec![issue.clone()], vec![issue.clone()]],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("recovered worktree should exist");

	state::write_run_activity_marker_for_process(&worktree.path, "run-1", 1, process::id())
		.expect("activity marker should write");
	tests::rewrite_run_activity_marker_host_boot_id(&worktree.path, "previous-boot");

	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, true)
		.expect("previous-boot recovery should succeed")
		.expect("previous-boot recovered lane should be selected for retry");

	assert_eq!(summary.issue_id, issue.id);
	assert_eq!(summary.dispatch_mode, orchestrator::IssueDispatchMode::Retry);
	assert!(
		state_store.lease_for_issue(&issue.id).expect("lease lookup should succeed").is_none(),
		"previous-boot markers must not reconstruct live leases even when the PID exists"
	);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn run_project_once_retries_recovered_worktree_from_reused_pid() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = recovery_terminal_support::sample_active_issue("In Progress");
	let tracker = FakeTracker::with_refresh_snapshots(
		vec![issue.clone()],
		vec![vec![issue.clone()], vec![issue.clone()]],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("recovered worktree should exist");

	state::write_run_activity_marker_for_process(&worktree.path, "run-1", 1, process::id())
		.expect("activity marker should write");
	tests::rewrite_run_activity_marker_process_start_identity(
		&worktree.path,
		"previous-process-start",
	);

	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, true)
		.expect("same-boot PID-reuse recovery should succeed")
		.expect("same-boot PID-reuse recovered lane should be selected for retry");

	assert_eq!(summary.issue_id, issue.id);
	assert_eq!(summary.dispatch_mode, orchestrator::IssueDispatchMode::Retry);
	assert!(
		state_store.lease_for_issue(&issue.id).expect("lease lookup should succeed").is_none(),
		"PID reuse must not reconstruct live leases when the process start identity changed"
	);
}

#[cfg(unix)]
#[test]
fn process_is_alive_handles_current_process_and_invalid_sentinel() {
	assert!(
		orchestrator::process_is_alive(process::id()),
		"current process should always be reported as alive"
	);
	assert!(
		!orchestrator::process_is_alive(u32::MAX),
		"sentinel pid values should never be treated as live processes"
	);
}

#[test]
fn run_project_once_clears_recovered_lease_when_marker_turns_stale() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = recovery_terminal_support::sample_active_issue("In Progress");
	let tracker = FakeTracker::with_refresh_snapshots(
		vec![issue.clone()],
		vec![vec![issue.clone()], vec![issue.clone()]],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("recovered worktree should exist");

	state::write_run_activity_marker(&worktree.path, "run-1", 1)
		.expect("fresh activity marker should write");

	let initial_summary =
		orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, true)
			.expect("initial recovery should succeed");

	assert!(
		initial_summary.is_none(),
		"fresh recovered activity should block redispatch and reconstruct the live lease"
	);
	assert_eq!(
		state_store
			.lease_for_issue(&issue.id)
			.expect("lease lookup should succeed")
			.expect("recovered lease should exist")
			.run_id(),
		"run-1"
	);

	state::write_run_activity_marker_for_process(&worktree.path, "run-1", 1, u32::MAX)
		.expect("stale activity marker should write");

	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, true)
		.expect("stale recovery should succeed")
		.expect("stale recovered lease should no longer block retry planning");

	assert_eq!(summary.issue_id, issue.id);
	assert_eq!(summary.dispatch_mode, orchestrator::IssueDispatchMode::Retry);
	assert!(
		state_store.lease_for_issue(&issue.id).expect("lease lookup should succeed").is_none(),
		"stale recovered markers should clear the reconstructed lease before retry planning"
	);
}

#[test]
fn run_project_once_skips_recovered_terminal_guarded_worktree_after_empty_state_startup() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = recovery_terminal_support::sample_active_issue_without_needs_attention_team_label(
		"In Progress",
	);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("recovered worktree should exist");

	fs::write(
		worktree.path.join(TERMINAL_GUARD_MARKER_FILE),
		"run_id=pub-101-attempt-1-123\nattempt_number=1\n",
	)
	.expect("terminal guard marker should write");

	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, true)
		.expect("recovery should succeed");

	assert!(
		summary.is_none(),
		"restart recovery should not redispatch retained lanes guarded by a terminal marker"
	);
	assert!(
		state_store
			.worktree_for_issue(&issue.id)
			.expect("worktree lookup should succeed")
			.is_some(),
		"worktree mapping should still be reconstructed for guarded retained lanes"
	);
}

#[test]
fn run_project_once_clears_terminal_queued_lane_labels_without_dispatch() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let issue = tests::sample_issue("Done", &[active_label.as_str()]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, false)
		.expect("terminal queued cleanup should succeed");

	assert!(summary.is_none(), "terminal queued issues should not dispatch");
	assert_eq!(
		tracker.label_removals.borrow().as_slice(),
		[
			(issue.id.clone(), vec![String::from("label-active")]),
			(issue.id.clone(), vec![String::from("label-queued")]),
		]
	);
}

#[test]
fn run_project_once_dry_run_keeps_terminal_queued_lane_labels() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let issue = tests::sample_issue("Done", &[active_label.as_str()]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, true)
		.expect("terminal queued dry run should succeed");

	assert!(summary.is_none(), "terminal queued dry run should not dispatch");
	assert!(
		tracker.label_removals.borrow().is_empty(),
		"dry run should not mutate terminal queued labels"
	);
}

#[test]
fn run_project_once_preserves_terminal_recovered_worktree_without_prior_state_when_review_handoff_is_missing()
 {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = recovery_terminal_support::sample_active_issue("Done");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("terminal retained worktree should be created");
	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, false)
		.expect("reconciliation should finish cleanly");

	assert!(
		summary.is_none(),
		"blocked retained closeout with missing handoff should not redispatch during recovery"
	);
	assert!(
		worktree.path.exists(),
		"terminal recovery should preserve the retained closeout worktree on disk for manual intervention"
	);
	assert!(
		state_store
			.worktree_for_issue(&issue.id)
			.expect("worktree lookup should succeed")
			.is_some(),
		"terminal recovery should preserve the retained closeout worktree mapping when review handoff is missing"
	);
}

#[test]
fn run_project_once_clears_stale_completed_closeout_lease_but_keeps_worktree() {
	let (_temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(
		&base_config,
		"DECODEX_TEST_MISSING_STARTUP_DELIVERY_CLOSEOUT_GITHUB_TOKEN",
	);
	let issue = recovery_terminal_support::sample_active_issue("Done");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("retained closeout worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let run_id = "run-closeout-startup";
	let pr_url = "https://github.com/hack-ink/decodex/pull/178";

	tests::seed_review_handoff_marker_value(
		&state_store,
		config.service_id(),
		&issue.id,
		&tests::sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
	);

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease(config.service_id(), &issue.id, run_id, &issue.state.name)
		.expect("stale lease should record");
	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree mapping should record");

	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, false)
		.expect("startup reconciliation should succeed");

	assert!(
		summary.is_none(),
		"blocked retained closeout should not redispatch during startup recovery"
	);
	assert!(
		state_store.lease_for_issue(&issue.id).expect("lease lookup should succeed").is_none(),
		"startup reconciliation should clear stale completed closeout leases"
	);
	assert_eq!(
		state_store
			.run_attempt(run_id)
			.expect("run attempt lookup should succeed")
			.expect("run attempt should still exist")
			.status(),
		"interrupted"
	);
	assert!(
		state_store
			.worktree_for_issue(&issue.id)
			.expect("worktree lookup should succeed")
			.is_some(),
		"startup reconciliation should preserve the retained closeout worktree mapping"
	);
	assert!(
		worktree.path.exists(),
		"startup reconciliation should leave the retained closeout worktree on disk"
	);
}

#[test]
fn run_project_once_preserves_fresh_completed_closeout_lease() {
	let (_temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(
		&base_config,
		"DECODEX_TEST_MISSING_STARTUP_DELIVERY_CLOSEOUT_GITHUB_TOKEN",
	);
	let issue = recovery_terminal_support::sample_active_issue("Done");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("retained closeout worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let run_id = "run-closeout-fresh-startup";
	let pr_url = "https://github.com/hack-ink/decodex/pull/178";

	tests::seed_review_handoff_marker_value(
		&state_store,
		config.service_id(),
		&issue.id,
		&tests::sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
	);
	state::write_run_activity_marker(&worktree.path, run_id, 1)
		.expect("fresh activity marker should write");

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease(config.service_id(), &issue.id, run_id, &issue.state.name)
		.expect("fresh lease should record");
	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree mapping should record");

	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, false)
		.expect("startup reconciliation should succeed");

	assert!(
		summary.is_none(),
		"fresh retained closeout activity should block redispatch during startup recovery"
	);
	assert_eq!(
		state_store
			.lease_for_issue(&issue.id)
			.expect("lease lookup should succeed")
			.expect("fresh retained closeout lease should survive")
			.run_id(),
		run_id
	);
	assert_eq!(
		state_store
			.run_attempt(run_id)
			.expect("run attempt lookup should succeed")
			.expect("run attempt should still exist")
			.status(),
		"running"
	);
	assert!(
		state_store
			.worktree_for_issue(&issue.id)
			.expect("worktree lookup should succeed")
			.is_some(),
		"startup reconciliation should preserve the retained closeout worktree mapping"
	);
}

#[test]
fn run_project_once_preserves_completed_unmerged_closeout_worktree() {
	let (temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(&base_config, "HOME");
	let issue = recovery_terminal_support::sample_active_issue("Done");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("retained closeout worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let run_id = "run-closeout-open-pr-startup";
	let pr_url = "https://github.com/hack-ink/decodex/pull/179";
	let _path_guard = recovery_terminal_support::install_fake_open_pr_gh_response(
		&temp_dir, &worktree, pr_url, &head_oid,
	);

	tests::seed_review_handoff_marker_value(
		&state_store,
		config.service_id(),
		&issue.id,
		&tests::sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
	);

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease(config.service_id(), &issue.id, run_id, &issue.state.name)
		.expect("stale lease should record");
	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree mapping should record");

	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, false)
		.expect("startup reconciliation should succeed");

	assert!(
		summary.is_none(),
		"completed retained closeout with an open PR should stay blocked during startup recovery"
	);
	assert!(
		state_store.lease_for_issue(&issue.id).expect("lease lookup should succeed").is_none(),
		"startup reconciliation should clear stale completed closeout leases when the PR is still open"
	);
	assert_eq!(
		state_store
			.run_attempt(run_id)
			.expect("run attempt lookup should succeed")
			.expect("run attempt should still exist")
			.status(),
		"interrupted"
	);
	assert!(
		state_store
			.worktree_for_issue(&issue.id)
			.expect("worktree lookup should succeed")
			.is_some(),
		"startup reconciliation should preserve the retained closeout worktree mapping until the PR merges"
	);
	assert!(
		worktree.path.exists(),
		"startup reconciliation should leave the retained closeout worktree on disk while waiting for merge"
	);
}

#[test]
fn run_project_once_skips_recovered_worktree_without_service_active_label() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("In Progress", &[]);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());

	worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("foreign retained worktree should exist");

	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, true)
		.expect("recovery should succeed");

	assert!(
		summary.is_none(),
		"recovery should skip retained worktrees that are not explicitly owned by this service"
	);
	assert!(
		state_store
			.worktree_for_issue(&issue.id)
			.expect("worktree lookup should succeed")
			.is_none(),
		"foreign retained worktrees should not be reconstructed into local service state"
	);
	assert!(
		state_store.lease_for_issue(&issue.id).expect("lease lookup should succeed").is_none(),
		"foreign retained worktrees should not rebuild service leases"
	);
}

#[test]
fn run_project_once_recovers_worktree_when_identifier_lookup_labels_are_truncated() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let listed_issue = recovery_terminal_support::sample_active_issue("In Progress");
	let mut identifier_lookup_issue = listed_issue.clone();

	identifier_lookup_issue.labels_complete = false;

	identifier_lookup_issue.labels.retain(|label| label.name != active_label);

	let tracker = FakeTracker::with_refresh_snapshots(
		vec![listed_issue.clone()],
		vec![vec![listed_issue.clone()]],
	)
	.with_identifier_lookup_issues(vec![identifier_lookup_issue]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let expected_worktree = worktree_manager
		.ensure_worktree(&listed_issue.identifier, false)
		.expect("recovered worktree should be created")
		.path;
	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, true)
		.expect("recovery should succeed")
		.expect("ambiguous label pagination should still recover the owned retained lane");

	assert_eq!(summary.issue_id, listed_issue.id);
	assert_eq!(summary.dispatch_mode, orchestrator::IssueDispatchMode::Retry);
	assert_eq!(summary.worktree_path, expected_worktree);
}

#[test]
fn recovery_skip_cache_suppresses_repeated_unowned_worktree_lookup() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("Todo", &[]);
	let tracker = FakeTracker::new(Vec::new()).with_identifier_lookup_issues(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_path = config.worktree_root().join(&issue.identifier);
	let mut skip_cache = RecoverableWorktreeSkipCache::default();

	fs::create_dir_all(&worktree_path).expect("stale worktree directory should exist");

	let first = orchestrator::recover_runtime_state_from_tracker_and_worktrees_with_skip_cache(
		&tracker,
		&config,
		&workflow,
		&state_store,
		Some(&mut skip_cache),
	)
	.expect("first recovery probe should succeed");
	let second = orchestrator::recover_runtime_state_from_tracker_and_worktrees_with_skip_cache(
		&tracker,
		&config,
		&workflow,
		&state_store,
		Some(&mut skip_cache),
	)
	.expect("cached recovery probe should succeed");
	let identifier_queries = tracker.identifier_queries.borrow();

	assert!(first.recoverable_issues.is_empty());
	assert!(second.recoverable_issues.is_empty());
	assert_eq!(identifier_queries.len(), 1);
	assert_eq!(identifier_queries[0], issue.identifier);
	assert!(
		tracker.refresh_queries.borrow().is_empty(),
		"empty known issue sets should not call tracker refresh"
	);
	assert!(
		tracker.label_queries.borrow().is_empty(),
		"complete issue labels should not need server confirmation"
	);
}

#[test]
fn live_run_skips_issue_that_becomes_ineligible_after_worktree_prepare() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let listed_issue = tests::sample_issue("Todo", &[]);
	let tracker = FakeTracker::with_refresh_snapshots(
		vec![listed_issue.clone()],
		vec![vec![], vec![listed_issue.clone()], vec![tests::sample_issue("In Progress", &[])]],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, false)
		.expect("run once should succeed");

	assert!(summary.is_none());
	assert!(
		state_store.lease_for_issue(&listed_issue.id).expect("lease lookup should work").is_none()
	);
	assert!(
		state_store
			.worktree_for_issue(&listed_issue.id)
			.expect("worktree lookup should work")
			.is_some()
	);
	assert!(tracker.comments.borrow().is_empty());
}

#[test]
fn live_run_clears_claimed_lease_when_refresh_fails_after_worktree_prepare() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let listed_issue = tests::sample_issue("Todo", &[]);
	let tracker =
		FakeTracker::with_refresh_error(vec![listed_issue.clone()], "transient refresh failure");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let error = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, false)
		.expect_err("run once should propagate refresh failure");

	assert!(
		error.to_string().contains("transient refresh failure"),
		"error should surface the refresh failure"
	);
	assert!(
		state_store.lease_for_issue(&listed_issue.id).expect("lease lookup should work").is_none()
	);
}

#[test]
fn run_project_once_ignores_fresh_marker_for_exited_process() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = recovery_terminal_support::sample_active_issue("In Progress");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("recovered worktree should exist");
	let exited_process_id = u32::MAX;

	state::write_run_activity_marker_for_process(&worktree.path, "run-1", 1, exited_process_id)
		.expect("activity marker should write");

	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, true)
		.expect("recovery should succeed")
		.expect("dead process marker should not block retry planning");

	assert_eq!(summary.issue_id, issue.id);
	assert_eq!(summary.dispatch_mode, orchestrator::IssueDispatchMode::Retry);
	assert!(
		state_store.lease_for_issue(&issue.id).expect("lease lookup should succeed").is_none(),
		"dead marker recovery should not reconstruct a live lease"
	);
}

#[test]
fn idle_daemon_recovery_reconstructs_completed_closeout_worktree_mapping() {
	let (_temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(
		&base_config,
		"DECODEX_TEST_MISSING_DAEMON_DELIVERY_CLOSEOUT_GITHUB_TOKEN",
	);
	let issue = recovery_terminal_support::sample_active_issue("Done");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("retained closeout worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/178";

	tests::seed_review_handoff_marker_value(
		&state_store,
		config.service_id(),
		&issue.id,
		&tests::sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
	);
	orchestrator::recover_and_reconcile_idle_daemon_state(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&worktree_manager,
		None,
	)
	.expect("idle daemon recovery should succeed");

	assert!(
		state_store
			.worktree_for_issue(&issue.id)
			.expect("worktree lookup should succeed")
			.is_some(),
		"idle daemon recovery should reconstruct retained closeout worktree mappings from disk"
	);
	assert!(
		state_store.lease_for_issue(&issue.id).expect("lease lookup should succeed").is_none(),
		"blocked retained closeout recovery should not invent a live lease without fresh activity"
	);
}
