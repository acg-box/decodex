use time::OffsetDateTime;

use crate::{
	agent::RUN_LEASE_IDLE_TIMEOUT,
	orchestrator::{
		self, RunLeaseDisposition,
		tests::{self, FakeTracker},
	},
	state::StateStore,
	worktree::WorktreeManager,
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

#[test]
fn run_lease_reconciliation_supersedes_stale_lease_for_newer_attempt() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue_with_sort_fields(
		"issue-superseded-lease",
		"PUB-207",
		"In Progress",
		&[],
		Some(3),
		"2026-03-13T04:16:17.133Z",
	);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let stale_run_id = "run-superseded-lease-1";
	let newer_run_id = "run-superseded-lease-2";
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());

	state_store
		.record_run_attempt(stale_run_id, &issue.id, 1, "running")
		.expect("stale run should record");
	state_store
		.record_run_attempt(newer_run_id, &issue.id, 2, "succeeded")
		.expect("newer run should record");
	state_store
		.upsert_lease(config.service_id(), &issue.id, stale_run_id, "In Progress")
		.expect("stale lease should record");

	let actions = orchestrator::inspect_run_lease_reconciliation_at(
		&tracker,
		&config,
		&workflow,
		&state_store,
		None,
		OffsetDateTime::now_utc().unix_timestamp(),
	)
	.expect("run lease inspection should succeed");

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

	assert!(state_store.lease_for_issue(&issue.id).expect("lease lookup should succeed").is_none());
	assert_eq!(
		state_store
			.run_attempt(stale_run_id)
			.expect("run attempt lookup should succeed")
			.expect("stale run should exist")
			.status(),
		"interrupted"
	);
	assert!(
		tracker.comments.borrow().is_empty(),
		"superseded stale lease must not write needs-attention comments"
	);
}
