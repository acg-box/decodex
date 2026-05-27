use crate::agent::AppServerRunResult;

#[test]
fn completed_issue_thread_archive_candidates_include_prior_terminal_attempts() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("In Progress", &[]);

	state_store
		.record_run_attempt("run-old", &issue.id, 1, "failed")
		.expect("old attempt should record");
	state_store.update_run_thread("run-old", "thread-old").expect("old thread should attach");
	state_store
		.append_event("run-old", 1, "turn/completed", "{}")
		.expect("old event should record");
	state_store
		.record_run_attempt("run-current", &issue.id, 2, "succeeded")
		.expect("current attempt should record");
	state_store
		.update_run_thread("run-current", "thread-current")
		.expect("current thread should attach");
	state_store
		.append_event("run-current", 1, "turn/completed", "{}")
		.expect("current event should record");
	state_store
		.record_run_attempt("run-active", &issue.id, 3, "running")
		.expect("active attempt should record");
	state_store.update_run_thread("run-active", "thread-active").expect("active thread should attach");
	state_store
		.record_run_attempt("run-archived", &issue.id, 4, TERMINAL_GUARDED_RUN_STATUS)
		.expect("archived attempt should record");
	state_store
		.update_run_thread("run-archived", "thread-archived")
		.expect("archived thread should attach");
	state_store
		.append_event("run-archived", 1, "thread/archive", "{}")
		.expect("archive event should record");

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
		thread_id: String::from("thread-current"),
		turn_id: String::from("turn-current"),
		turn_count: 1,
		event_count: 1,
		final_output: String::new(),
		continuation_pending: false,
	};
	let candidates =
		super::completed_issue_thread_archive_candidates(&state_store, &issue_run, &run_result)
			.expect("archive candidates should load");
	let candidate_threads = candidates
		.iter()
		.map(|candidate| candidate.thread_id.as_str())
		.collect::<Vec<_>>();

	assert_eq!(candidate_threads, vec!["thread-current", "thread-old"]);
	assert_eq!(candidates[0].sequence_number, 2);
	assert_eq!(candidates[1].sequence_number, 2);
}
