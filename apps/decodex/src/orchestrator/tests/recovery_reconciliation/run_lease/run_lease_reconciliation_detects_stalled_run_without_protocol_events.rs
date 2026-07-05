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
fn run_lease_reconciliation_detects_stalled_run_without_protocol_events() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let stalled_issue = tests::sample_issue_with_sort_fields(
		"issue-stalled-no-events",
		"PUB-204",
		"In Progress",
		&[],
		Some(3),
		"2026-03-13T04:16:17.133Z",
	);
	let tracker = FakeTracker::new(vec![stalled_issue.clone()]);

	state_store
		.record_run_attempt(
			&format!("run-{}", stalled_issue.identifier),
			&stalled_issue.id,
			1,
			"running",
		)
		.expect("run attempt should record");
	state_store
		.upsert_lease(
			"pubfi",
			&stalled_issue.id,
			&format!("run-{}", stalled_issue.identifier),
			"In Progress",
		)
		.expect("lease should record");

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
		action.issue.id == stalled_issue.id
			&& matches!(
				action.disposition,
				RunLeaseDisposition::Stalled{ idle_for }
					if idle_for >= RUN_LEASE_IDLE_TIMEOUT
			)
	}));
}
