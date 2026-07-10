use crate::{
	orchestrator::{
		self, PhaseGoalKind, StateStore,
		tests::{self, TEST_SERVICE_ID},
	},
	state::{PROGRESS_CHECKPOINT_EVENT_TYPE, PROGRESS_CHECKPOINT_SCHEMA},
	tracker,
};

#[test]
fn cleared_checkpoint_allows_cross_attempt_phase_goal_inheritance() {
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let source_run_id = "pub-101-attempt-1";

	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			source_run_id,
			1,
			"phase_goal_next",
			serde_json::json!({
				"phase": "handoff_evidence",
			}),
		)
		.expect("phase goal next should record");
	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			source_run_id,
			1,
			PROGRESS_CHECKPOINT_EVENT_TYPE,
			serde_json::json!({
				"schema": PROGRESS_CHECKPOINT_SCHEMA,
				"record_version": 2,
				"blockers": ["repo-wide baseline requires separate authority"],
			}),
		)
		.expect("blocking checkpoint should record");
	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			source_run_id,
			1,
			PROGRESS_CHECKPOINT_EVENT_TYPE,
			serde_json::json!({
				"schema": PROGRESS_CHECKPOINT_SCHEMA,
				"record_version": 2,
				"blockers": [],
			}),
		)
		.expect("clearing checkpoint should record");

	assert_eq!(
		orchestrator::latest_open_issue_phase_goal_before_attempt(
			&config,
			&state_store,
			&issue.id,
			"pub-101-attempt-2",
			2,
		)
		.expect("phase goal inheritance should evaluate"),
		Some(PhaseGoalKind::HandoffEvidence)
	);
}
