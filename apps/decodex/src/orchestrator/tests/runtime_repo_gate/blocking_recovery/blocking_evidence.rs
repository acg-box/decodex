use crate::{
	orchestrator::{
		self, StateStore,
		tests::{self, TEST_SERVICE_ID},
	},
	state::{PROGRESS_CHECKPOINT_EVENT_TYPE, PROGRESS_CHECKPOINT_SCHEMA},
	tracker,
};

#[test]
fn blocking_lane_decision_evidence_ignores_incomplete_or_mismatched_checkpoint_contract() {
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");

	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			"run-unversioned-checkpoint",
			1,
			PROGRESS_CHECKPOINT_EVENT_TYPE,
			serde_json::json!({
				"schema": PROGRESS_CHECKPOINT_SCHEMA,
				"blockers": ["old payload must not retain runtime authority"],
			}),
		)
		.expect("unversioned checkpoint fixture should record");

	assert!(
		!orchestrator::issue_has_blocking_lane_decision_evidence(&config, &state_store, &issue.id,)
			.expect("blocking evidence should evaluate"),
		"checkpoint payloads without record_version must not satisfy the current contract"
	);

	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			"run-mismatched-checkpoint",
			1,
			PROGRESS_CHECKPOINT_EVENT_TYPE,
			serde_json::json!({
				"schema": PROGRESS_CHECKPOINT_SCHEMA,
				"record_version": 1,
				"blockers": ["mismatched version must not retain runtime authority"],
			}),
		)
		.expect("mismatched checkpoint fixture should record");

	assert!(
		!orchestrator::issue_has_blocking_lane_decision_evidence(&config, &state_store, &issue.id,)
			.expect("blocking evidence should evaluate"),
		"checkpoint payloads with mismatched record_version must not satisfy the current contract"
	);
}

#[test]
fn blocking_lane_decision_evidence_clears_after_new_unblocked_checkpoint() {
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-cleared-blocker";

	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			run_id,
			1,
			PROGRESS_CHECKPOINT_EVENT_TYPE,
			serde_json::json!({
				"schema": PROGRESS_CHECKPOINT_SCHEMA,
				"record_version": 2,
				"blockers": ["repo-wide baseline requires separate authority"],
			}),
		)
		.expect("blocking checkpoint should record");

	assert!(
		orchestrator::issue_has_blocking_lane_decision_evidence(&config, &state_store, &issue.id)
			.expect("blocking evidence should evaluate")
	);

	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			run_id,
			1,
			PROGRESS_CHECKPOINT_EVENT_TYPE,
			serde_json::json!({
				"schema": PROGRESS_CHECKPOINT_SCHEMA,
				"record_version": 2,
			}),
		)
		.expect("ordinary checkpoint should record");

	assert!(
		orchestrator::issue_has_blocking_lane_decision_evidence(&config, &state_store, &issue.id)
			.expect("blocking evidence should evaluate"),
		"checkpoint without an explicit empty blockers array must not clear older blockers"
	);

	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			run_id,
			1,
			PROGRESS_CHECKPOINT_EVENT_TYPE,
			serde_json::json!({
				"schema": PROGRESS_CHECKPOINT_SCHEMA,
				"record_version": 2,
				"blockers": [],
			}),
		)
		.expect("clearing checkpoint should record");

	assert!(
		!orchestrator::issue_has_blocking_lane_decision_evidence(&config, &state_store, &issue.id)
			.expect("blocking evidence should evaluate"),
		"latest unblocked checkpoint should clear older progress blockers"
	);
}

#[test]
fn blocking_lane_decision_evidence_prefers_kernel_projection_over_legacy_action() {
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-kernel-lane-decision";

	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			run_id,
			1,
			"lane_decision",
			serde_json::json!({
				"next_action": "needs_attention",
				"kernel_decision": {
					"decision_class": "retry_automatically",
					"command_intents": [{"kind": "schedule_retry"}],
				},
			}),
		)
		.expect("kernel retry decision should record");

	assert!(
		!orchestrator::issue_has_blocking_lane_decision_evidence(&config, &state_store, &issue.id)
			.expect("blocking evidence should evaluate"),
		"kernel decision must override stale compatibility action"
	);

	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			run_id,
			1,
			"lane_decision",
			serde_json::json!({
				"next_action": "retry_failure",
				"kernel_decision": {
					"decision_class": "manual_intervention_required",
					"command_intents": [{"kind": "request_manual_intervention"}],
				},
			}),
		)
		.expect("kernel manual decision should record");

	assert!(
		orchestrator::issue_has_blocking_lane_decision_evidence(&config, &state_store, &issue.id)
			.expect("blocking evidence should evaluate"),
		"kernel manual decision must block even when compatibility action is stale"
	);
}
