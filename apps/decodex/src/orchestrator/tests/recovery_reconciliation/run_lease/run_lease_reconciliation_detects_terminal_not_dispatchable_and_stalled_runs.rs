use time::OffsetDateTime;

use crate::{
	agent::RUN_LEASE_IDLE_TIMEOUT,
	orchestrator::{
		self, RunLeaseDisposition,
		tests::{self, FakeTracker},
	},
	state::StateStore,
};

#[test]
fn run_lease_reconciliation_detects_terminal_not_dispatchable_and_stalled_runs() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let terminal_issue = tests::sample_issue_with_sort_fields(
		"issue-terminal",
		"PUB-201",
		"Done",
		&[],
		Some(3),
		"2026-03-13T04:16:17.133Z",
	);
	let not_dispatchable_issue = tests::sample_issue_with_sort_fields(
		"issue-not-dispatchable",
		"PUB-202",
		"Blocked",
		&[],
		Some(3),
		"2026-03-13T04:16:17.133Z",
	);
	let stalled_issue = tests::sample_issue_with_sort_fields(
		"issue-stalled",
		"PUB-203",
		"In Progress",
		&[],
		Some(3),
		"2026-03-13T04:16:17.133Z",
	);
	let tracker = FakeTracker::new(vec![
		terminal_issue.clone(),
		not_dispatchable_issue.clone(),
		stalled_issue.clone(),
	]);

	for issue in [&terminal_issue, &not_dispatchable_issue, &stalled_issue] {
		state_store
			.record_run_attempt(&format!("run-{}", issue.identifier), &issue.id, 1, "running")
			.expect("run attempt should record");
		state_store
			.upsert_lease("pubfi", &issue.id, &format!("run-{}", issue.identifier), "In Progress")
			.expect("lease should record");
	}

	state_store
		.append_event(
			&format!("run-{}", stalled_issue.identifier),
			1,
			"thread/status/changed",
			"{\"status\":\"active\"}",
		)
		.expect("stalled issue protocol event should record");

	let now =
		OffsetDateTime::now_utc().unix_timestamp() + RUN_LEASE_IDLE_TIMEOUT.as_secs() as i64 + 1;
	let actions = orchestrator::inspect_run_lease_reconciliation_at(
		&tracker,
		&config,
		&workflow,
		&state_store,
		None,
		now,
	)
	.expect("run lease inspection should succeed");

	assert!(actions.iter().any(|action| {
		action.issue.id == terminal_issue.id
			&& matches!(action.disposition, orchestrator::RunLeaseDisposition::Terminal)
	}));
	assert!(actions.iter().any(|action| {
		action.issue.id == not_dispatchable_issue.id
			&& matches!(action.disposition, orchestrator::RunLeaseDisposition::NotDispatchable)
	}));
	assert!(actions.iter().any(|action| {
		action.issue.id == stalled_issue.id
			&& matches!(
			action.disposition,
			RunLeaseDisposition::Stalled{ idle_for }
				if idle_for >= RUN_LEASE_IDLE_TIMEOUT
			)
	}));
}
