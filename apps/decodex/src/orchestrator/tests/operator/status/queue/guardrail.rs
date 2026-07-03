use crate::orchestrator::tests::operator::status::{
	self, FakeTracker, ServiceConfig, StateStore, TrackerIssue, WorkflowDocument, orchestrator,
};

#[test]
fn queued_status_guardrail_requires_explicit_command_application() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let mut issue = status::sample_issue_with_sort_fields(
		"issue-blocked",
		"PUB-108",
		"Todo",
		&[],
		Some(1),
		"2026-03-13T04:16:17.133Z",
	);

	issue.blockers = vec![status::sample_blocker("issue-open", "PUB-105", "Todo")];

	assert_status_reads_keep_guardrail_checkpoint_empty(&config, &workflow, &state_store, &issue);

	assert!(
		state_store
			.loop_guardrail_checkpoint(config.service_id(), &issue.id, "dependency_program_stale")
			.expect("dependency guardrail checkpoint should read")
			.is_none(),
		"operator status reads must not mutate queue guardrail checkpoints"
	);

	apply_open_blocker_guardrail_observations(&config, &workflow, &state_store, &issue);

	let checkpoint = state_store
		.loop_guardrail_checkpoint(config.service_id(), &issue.id, "dependency_program_stale")
		.expect("dependency guardrail checkpoint should read")
		.expect("dependency guardrail checkpoint should exist");

	assert_eq!(checkpoint.consecutive_count(), 3);

	let tracker = FakeTracker::new(vec![issue.clone()]);
	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("stale blocker snapshot should build");
	let candidate = snapshot.queued_candidates.first().expect("blocked queued issue should exist");

	assert_eq!(candidate.reason, "dependency_program_stale");

	issue.blockers = vec![status::sample_blocker("issue-open", "PUB-105", "Done")];

	let tracker = FakeTracker::new(vec![issue.clone()]);
	let plan = orchestrator::build_queued_candidate_status_plan(
		&tracker,
		&config,
		&workflow,
		&state_store,
	)
	.expect("resolved blocker plan should build");
	let candidate = plan.statuses.first().expect("ready queued issue should exist");
	let command = plan
		.guardrail_commands
		.first()
		.expect("resolved blockers should request stale guardrail cleanup");

	assert_eq!(candidate.reason, "eligible_for_dispatch");
	assert_eq!(command.intent.kind.as_str(), "clear_loop_guardrail_checkpoint");
	assert_eq!(command.intent.idempotency_key, "issue-blocked:dependency_program_stale:clear");
	assert_eq!(
		command.intent.preconditions.iter().map(|fact| fact.as_str()).collect::<Vec<_>>(),
		vec!["open_tracker_blockers_resolved"]
	);
	assert_eq!(
		command.intent.expected_postconditions.iter().map(|fact| fact.as_str()).collect::<Vec<_>>(),
		vec!["loop_guardrail_checkpoint_cleared"]
	);

	orchestrator::apply_queued_candidate_guardrail_commands(
		&config,
		&workflow,
		&state_store,
		&plan.guardrail_commands,
	)
	.expect("clear guardrail command should apply");

	assert!(
		state_store
			.loop_guardrail_checkpoint(config.service_id(), &issue.id, "dependency_program_stale")
			.expect("cleared dependency checkpoint should read")
			.is_none()
	);

	issue.blockers = vec![status::sample_blocker("issue-open", "PUB-105", "Todo")];

	let tracker = FakeTracker::new(vec![issue.clone()]);
	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("recurring blocker snapshot should build");
	let candidate = snapshot.queued_candidates.first().expect("blocked queued issue should exist");

	assert_eq!(candidate.reason, "open_tracker_blockers");
	assert_eq!(candidate.blocker_identifiers, vec![String::from("PUB-105")]);
}

fn assert_status_reads_keep_guardrail_checkpoint_empty(
	config: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue: &TrackerIssue,
) {
	for _ in 0..3 {
		let tracker = FakeTracker::new(vec![issue.clone()]);
		let snapshot = orchestrator::build_live_operator_status_snapshot(
			&tracker,
			config,
			workflow,
			state_store,
			10,
		)
		.expect("snapshot should build");
		let candidate =
			snapshot.queued_candidates.first().expect("blocked queued issue should exist");

		assert_eq!(candidate.reason, "open_tracker_blockers");
		assert_eq!(candidate.classification, "blocked");
		assert_eq!(candidate.blocker_identifiers, vec![String::from("PUB-105")]);
	}
}

fn apply_open_blocker_guardrail_observations(
	config: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue: &TrackerIssue,
) {
	for _ in 0..3 {
		let tracker = FakeTracker::new(vec![issue.clone()]);
		let plan = orchestrator::build_queued_candidate_status_plan(
			&tracker,
			config,
			workflow,
			state_store,
		)
		.expect("queued status plan should build");
		let command = plan
			.guardrail_commands
			.first()
			.expect("open blockers should request a guardrail observation");

		assert_eq!(command.intent.kind.as_str(), "observe_loop_guardrail_checkpoint");
		assert_eq!(
			command.intent.idempotency_key,
			"issue-blocked:dependency_program_stale:observe"
		);
		assert_eq!(
			command.intent.preconditions.iter().map(|fact| fact.as_str()).collect::<Vec<_>>(),
			vec!["open_tracker_blockers_present"]
		);
		assert_eq!(
			command
				.intent
				.expected_postconditions
				.iter()
				.map(|fact| fact.as_str())
				.collect::<Vec<_>>(),
			vec!["loop_guardrail_checkpoint_observed"]
		);

		orchestrator::apply_queued_candidate_guardrail_commands(
			config,
			workflow,
			state_store,
			&plan.guardrail_commands,
		)
		.expect("guardrail command application should succeed");
	}
}
