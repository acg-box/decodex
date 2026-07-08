use crate::orchestrator::tests::operator::status::running_lanes::{self, StateStore, orchestrator};

#[test]
fn keeps_current_lanes_when_recent_limited() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let first_issue = running_lanes::sample_issue_with_sort_fields(
		"issue-1",
		"PUB-101",
		"Todo",
		&[],
		Some(3),
		"2026-03-13T04:16:17.133Z",
	);
	let second_issue = running_lanes::sample_issue_with_sort_fields(
		"issue-2",
		"PUB-102",
		"Todo",
		&[],
		Some(3),
		"2026-03-13T04:17:17.133Z",
	);

	for (run_id, issue, branch_suffix) in
		[("run-1", &first_issue, "101"), ("run-2", &second_issue, "102")]
	{
		state_store
			.record_run_attempt(run_id, &issue.id, 1, "running")
			.expect("run attempt should record");
		state_store
			.upsert_lease("pubfi", &issue.id, run_id, "In Progress")
			.expect("lease should record");
		state_store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				&format!("x/pubfi-pub-{branch_suffix}"),
				&config.worktree_root().join(&issue.identifier).display().to_string(),
			)
			.expect("worktree should record");
	}

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 1)
		.expect("snapshot should build");

	assert_eq!(snapshot.run_limit, 1);
	assert_eq!(snapshot.recent_runs.len(), 2);
	assert_eq!(snapshot.current_lanes.len(), 2);
	assert!(snapshot.current_lanes.iter().all(|run| run.run_lease));
}
