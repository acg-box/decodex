use std::{fs, process::Command};

use time::OffsetDateTime;

use crate::{
	orchestrator::{
		self, ChildExitRetryContext, ChildRunRef, IssueDispatchMode, RetryQueue, StateStore, tests,
		tests::{FakeTracker, retry_scheduling::support},
	},
	state,
};

#[test]
fn schedule_retry_after_child_exit_preserves_specific_retry_schedule_kind_for_failure_retry() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = support::sample_service_owned_issue("In Progress");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-1";
	let worktree_path = config.worktree_root().join("PUB-101");

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "failed")
		.expect("run attempt should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");
	state::write_run_retry_schedule(
		&worktree_path,
		run_id,
		1,
		"git_lock_contention",
		OffsetDateTime::now_utc().unix_timestamp() + 30,
	)
	.expect("specific retry schedule should write");

	let exit_status =
		Command::new("sh").args(["-c", "exit 1"]).status().expect("failure exit should run");
	let mut retry_queue = RetryQueue::default();

	orchestrator::schedule_retry_after_child_exit(
		ChildExitRetryContext {
			retry_queue: &mut retry_queue,
			tracker: &tracker,
			project: &config,
			workflow: &workflow,
			state_store: &state_store,
		},
		ChildRunRef { issue_id: &issue.id, run_id, attempt_number: 1 },
		issue.project_slug.as_deref().expect("sample issue should carry a project slug"),
		&issue.state.name,
		IssueDispatchMode::Retry,
		exit_status,
	)
	.expect("failure retry should schedule");

	let marker = state::read_run_activity_marker_snapshot(&worktree_path)
		.expect("retry schedule should remain readable")
		.expect("retry marker should exist");
	let entry = retry_queue.entries.get(&issue.id).expect("retry entry should exist for the issue");

	assert_eq!(entry.kind, orchestrator::RetryKind::Failure);
	assert_eq!(marker.retry_kind(), Some("git_lock_contention"));
}
