use tempfile::TempDir;

use crate::{
	agent::{AppServerCapabilityPreflightReport, AppServerRunResult},
	orchestrator::{
		self, IssueDispatchMode, IssueRunPlan, StateStore, TERMINAL_GUARDED_RUN_STATUS, tests,
	},
	worktree::WorktreeSpec,
};

#[test]
fn completed_issue_thread_archive_candidates_include_prior_terminal_attempts() {
	let project_id = "decodex";
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("In Progress", &[]);

	state_store
		.record_lane_run_attempt(project_id, "run-old", &issue.id, 1, "failed")
		.expect("old attempt should record");
	state_store.update_run_thread("run-old", "thread-old").expect("old thread should attach");
	state_store
		.append_event("run-old", 1, "turn/completed", "{}")
		.expect("old event should record");
	state_store
		.record_lane_run_attempt(project_id, "run-current", &issue.id, 2, "succeeded")
		.expect("current attempt should record");
	state_store
		.update_run_thread("run-current", "thread-current")
		.expect("current thread should attach");
	state_store
		.append_event("run-current", 1, "turn/completed", "{}")
		.expect("current event should record");
	state_store
		.record_lane_run_attempt(project_id, "run-active", &issue.id, 3, "running")
		.expect("active attempt should record");
	state_store
		.update_run_thread("run-active", "thread-active")
		.expect("active thread should attach");
	state_store
		.record_lane_run_attempt(
			project_id,
			"run-archived",
			&issue.id,
			4,
			TERMINAL_GUARDED_RUN_STATUS,
		)
		.expect("archived attempt should record");
	state_store
		.update_run_thread("run-archived", "thread-archived")
		.expect("archived thread should attach");
	state_store
		.append_event("run-archived", 1, "thread/archive", "{}")
		.expect("archive event should record");
	state_store
		.record_lane_run_attempt("other-project", "run-other", &issue.id, 5, "failed")
		.expect("colliding project attempt should record");
	state_store
		.update_run_thread("run-other", "thread-other")
		.expect("colliding thread should attach");
	state_store
		.append_event("run-other", 1, "turn/completed", "{}")
		.expect("colliding event should record");

	let issue_run = IssueRunPlan {
		issue,
		issue_state: String::from("In Progress"),
		initial_issue_state: String::from("Todo"),
		worktree: WorktreeSpec {
			branch_name: String::from("xy/test"),
			issue_identifier: String::from("PUB-101"),
			path: temp_dir.path().join("PUB-101"),
			reused_existing: false,
		},
		retry_project_slug: String::from("pubfi"),
		dispatch_mode: IssueDispatchMode::Normal,
		attempt_number: 2,
		run_id: String::from("run-current"),
		retry_budget_base: 0,
	};
	let run_result = AppServerRunResult {
		user_agent: String::from("codex-test"),
		capability_preflight: AppServerCapabilityPreflightReport::new(),
		thread_id: String::from("thread-current"),
		turn_id: String::from("turn-current"),
		turn_count: 1,
		event_count: 1,
		final_output: String::new(),
		continuation_pending: false,
		phase_goal_status: None,
	};
	let candidates = orchestrator::completed_issue_thread_archive_candidates(
		&state_store,
		project_id,
		&issue_run,
		&run_result,
	)
	.expect("archive candidates should load");
	let candidate_threads =
		candidates.iter().map(|candidate| candidate.thread_id.as_str()).collect::<Vec<_>>();

	assert_eq!(candidate_threads, vec!["thread-current", "thread-old"]);
	assert_eq!(candidates[0].sequence_number, 2);
	assert_eq!(candidates[1].sequence_number, 2);
}

#[test]
fn terminal_thread_archive_backlog_candidates_scan_project_terminal_runs() {
	let state_store = StateStore::open_in_memory().expect("state store should open");

	for (project, issue_id, run_id, status, thread_id) in [
		("decodex", "issue-succeeded", "run-succeeded", "succeeded", "thread-succeeded"),
		("decodex", "issue-failed", "run-failed", "failed", "thread-failed"),
		("decodex", "issue-terminated", "run-terminated", "terminated", "thread-terminated"),
		("decodex", "issue-running", "run-running", "running", "thread-running"),
		("other", "issue-other", "run-other", "succeeded", "thread-other"),
		("decodex", "issue-archived", "run-archived", "succeeded", "thread-archived"),
		("decodex", "issue-discarded", "run-discarded", "succeeded", "thread-discarded"),
	] {
		state_store
			.try_acquire_lease(project, issue_id, run_id, "In Progress")
			.expect("lease should record project ownership");
		state_store.record_run_attempt(run_id, issue_id, 1, status).expect("attempt should record");
		state_store.update_run_thread(run_id, thread_id).expect("thread should attach");
	}

	state_store
		.append_event("run-archived", 1, "thread/archive", "{}")
		.expect("archive event should record");
	state_store
		.append_event("run-discarded", 1, "thread/archive/discarded", "{}")
		.expect("discarded archive event should record");

	let candidates =
		orchestrator::terminal_thread_archive_backlog_candidates(&state_store, "decodex")
			.expect("backlog candidates should load");
	let mut candidate_threads =
		candidates.iter().map(|candidate| candidate.thread_id.as_str()).collect::<Vec<_>>();

	candidate_threads.sort_unstable();

	assert_eq!(candidate_threads, vec!["thread-failed", "thread-succeeded", "thread-terminated"]);
}

#[test]
fn terminal_thread_archive_reconciler_records_backlog_archive_events() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue_id = "issue-succeeded";
	let run_id = "run-succeeded";

	state_store
		.try_acquire_lease(config.service_id(), issue_id, run_id, "In Progress")
		.expect("lease should record project ownership");
	state_store
		.record_run_attempt(run_id, issue_id, 1, "succeeded")
		.expect("attempt should record");
	state_store.update_run_thread(run_id, "thread-succeeded").expect("thread should attach");

	orchestrator::reconcile_terminal_thread_archive_backlog_best_effort(
		&config,
		&workflow,
		&state_store,
	);

	assert!(
		state_store
			.run_has_protocol_event(run_id, "thread/archive")
			.expect("archive event lookup should succeed")
	);
}
