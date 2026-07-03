use crate::state::StateStore;

pub(in crate::recovery::tests::stale_active) fn append_app_server_no_progress_failure_evidence(
	store: &StateStore,
	issue_id: &str,
) {
	for (event_type, payload) in [
		(
			"loop_guardrail_checkpoint",
			serde_json::json!({
				"checkpoint_attempt_number": 1,
				"checkpoint_run_id": "run-1626",
				"consecutive_count": 1,
				"details": serde_json::json!({
					"branch_delta_present": false,
					"effective_delta_present": false,
					"reason": "no_effective_diff",
					"source_error_class": "app_server_turn_failed",
				})
				.to_string(),
				"fingerprint": "empty:empty",
				"reason": "no_effective_diff",
				"schema": "decodex.loop_guardrail_checkpoint/1",
				"source_error_class": "app_server_turn_failed",
				"threshold": 3,
			}),
		),
		(
			"harness_outcome",
			serde_json::json!({
				"authority_boundary": {
					"dispositions": [],
					"failed_check_count": 0,
					"improvement_signal_count": 0,
				},
				"contracts": [],
				"execution_programs": [],
				"linear_projection": {
					"event_types": ["run_started"],
					"final_error_class": null,
					"final_event_type": "run_started",
					"final_terminal_path": null,
				},
				"manual_attention": null,
				"phase_goal_outcomes": [{
					"event_type": "phase_goal_set",
					"phase": "implement_to_validation_ready",
					"status": "active",
				}],
				"pr_lifecycle": {
					"outcome": "retryable_failure",
					"pr_urls": [],
				},
				"record_version": 1,
				"repair": {
					"attempt_number": 1,
					"repair_attempt_observed": false,
					"repair_phase_events": 0,
				},
				"review": {
					"accepted_finding_count": 0,
					"nonclean_rounds": 0,
					"rejected_finding_count": 0,
					"statuses": [],
				},
				"schema": "decodex.harness_outcome/1",
				"source": {
					"attempt_number": 1,
					"issue_id": issue_id,
					"issue_identifier": "PUB-1626",
					"outcome": "retryable_failure",
					"project_id": "pubfi",
					"run_id": "run-1626",
					"source_intents": [],
				},
				"validation": {
					"failure_classes": [],
					"failure_count": 0,
					"result": "not_recorded",
				},
			}),
		),
	] {
		store
			.append_private_execution_event("pubfi", issue_id, "run-1626", 1, event_type, payload)
			.expect("private no-progress failure evidence should record");
	}
}

pub(in crate::recovery::tests::stale_active) fn append_no_diff_guardrail_event(
	store: &StateStore,
	issue_id: &str,
	branch_delta_present: bool,
	effective_delta_present: bool,
) {
	append_no_diff_guardrail_event_with_source_error_class(
		store,
		issue_id,
		branch_delta_present,
		effective_delta_present,
		Some("app_server_turn_failed"),
	);
}

pub(in crate::recovery::tests::stale_active) fn append_no_diff_guardrail_event_with_source_error_class(
	store: &StateStore,
	issue_id: &str,
	branch_delta_present: bool,
	effective_delta_present: bool,
	source_error_class: Option<&str>,
) {
	store
		.append_private_execution_event(
			"pubfi",
			issue_id,
			"run-1626",
			1,
			"loop_guardrail_checkpoint",
			serde_json::json!({
				"details": serde_json::json!({
					"branch_delta_present": branch_delta_present,
					"effective_delta_present": effective_delta_present,
				})
				.to_string(),
				"reason": "no_effective_diff",
				"schema": "decodex.loop_guardrail_checkpoint/1",
				"source_error_class": source_error_class,
			}),
		)
		.expect("private guardrail evidence should record");
}

pub(in crate::recovery::tests::stale_active) fn append_harness_outcome_with_pr_progress(
	store: &StateStore,
	issue_id: &str,
) {
	store
		.append_private_execution_event(
			"pubfi",
			issue_id,
			"run-1626",
			1,
			"harness_outcome",
			serde_json::json!({
				"manual_attention": null,
				"pr_lifecycle": {
					"outcome": "retryable_failure",
					"pr_urls": ["https://github.com/hack-ink/pubfi/pull/1631"],
				},
				"review": {
					"accepted_finding_count": 0,
					"nonclean_rounds": 0,
					"rejected_finding_count": 0,
					"statuses": [],
				},
				"schema": "decodex.harness_outcome/1",
				"source": {
					"outcome": "retryable_failure",
				},
				"validation": {
					"failure_classes": [],
					"failure_count": 0,
					"result": "not_recorded",
				},
			}),
		)
		.expect("private harness progress evidence should record");
}

pub(in crate::recovery::tests::stale_active) fn append_harness_outcome_with_review_progress(
	store: &StateStore,
	issue_id: &str,
) {
	store
		.append_private_execution_event(
			"pubfi",
			issue_id,
			"run-1626",
			1,
			"harness_outcome",
			serde_json::json!({
				"contracts": [],
				"execution_programs": [],
				"manual_attention": null,
				"pr_lifecycle": {
					"outcome": "retryable_failure",
					"pr_urls": [],
				},
				"review": {
					"accepted_finding_count": 1,
					"nonclean_rounds": 0,
					"rejected_finding_count": 0,
					"statuses": [],
				},
				"schema": "decodex.harness_outcome/1",
				"source": {
					"outcome": "retryable_failure",
				},
				"validation": {
					"failure_classes": [],
					"failure_count": 0,
					"result": "not_recorded",
				},
			}),
		)
		.expect("private harness review progress evidence should record");
}

pub(in crate::recovery::tests::stale_active) fn append_harness_outcome_with_validation_progress(
	store: &StateStore,
	issue_id: &str,
) {
	store
		.append_private_execution_event(
			"pubfi",
			issue_id,
			"run-1626",
			1,
			"harness_outcome",
			serde_json::json!({
				"contracts": [],
				"execution_programs": [],
				"manual_attention": null,
				"pr_lifecycle": {
					"outcome": "retryable_failure",
					"pr_urls": [],
				},
				"review": {
					"accepted_finding_count": 0,
					"nonclean_rounds": 0,
					"rejected_finding_count": 0,
					"statuses": [],
				},
				"schema": "decodex.harness_outcome/1",
				"source": {
					"outcome": "retryable_failure",
				},
				"validation": {
					"failure_classes": ["repo_gate_verify_failed"],
					"failure_count": 1,
					"result": "failed",
				},
			}),
		)
		.expect("private harness validation progress evidence should record");
}

pub(in crate::recovery::tests::stale_active) fn append_phase_goal_recovery_event(
	store: &StateStore,
	issue_id: &str,
	phase: &str,
	source_error_class: &str,
) {
	store
		.append_private_execution_event(
			"pubfi",
			issue_id,
			"run-1626",
			1,
			"phase_goal_recovery",
			serde_json::json!({
					"schema": "decodex.phase_goal_signal/1",
					"phase": phase,
					"signal": "phase_goal_recovered",
					"payload": {
						"nextPhase": "handoff_evidence",
					"sourceErrorClass": source_error_class,
					"sourceErrorMessage": "runtime failure",
				},
			}),
		)
		.expect("private phase goal recovery evidence should record");
}
