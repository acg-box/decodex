use crate::{
	config::ServiceConfig,
	orchestrator::tests::{
		FakeTracker,
		operator::status::running_lanes::autonomy_lineage::fixtures::{
			AUTONOMY_RUN_ID, SERVICE_ID,
		},
	},
	program_intake::{self, GoalIntakeRunRequest},
	state::StateStore,
	tracker::TrackerIssue,
	workflow::WorkflowDocument,
};

pub(super) fn seed_autonomy_run(state_store: &StateStore, issue: &TrackerIssue) {
	state_store
		.record_run_attempt(AUTONOMY_RUN_ID, &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease(SERVICE_ID, &issue.id, AUTONOMY_RUN_ID, "In Progress")
		.expect("lease should record");
	state_store
		.append_event(AUTONOMY_RUN_ID, 1, "turn/completed", "{\"turn\":\"1\"}")
		.expect("event should record");
}

pub(super) fn apply_goal_intake(
	state_store: &StateStore,
	config: &ServiceConfig,
	workflow: &WorkflowDocument,
	issue: &TrackerIssue,
	decision_contract_id: &str,
) {
	let tracker = FakeTracker::new(vec![issue.clone()]);

	program_intake::run_goal_intake(GoalIntakeRunRequest {
		state_store,
		tracker: &tracker,
		config,
		workflow,
		contract_id: decision_contract_id,
		team_issue_identifier: None,
		dry_run: false,
		apply: true,
	})
	.expect("goal intake should apply");
}
