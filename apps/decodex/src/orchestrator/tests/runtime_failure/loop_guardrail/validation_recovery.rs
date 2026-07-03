use crate::orchestrator::{
	RepoGateFailure,
	tests::{
		self,
		runtime_failure::{
			self, LoopGuardrailReason, LoopGuardrailStopRequested, RepoGateFailureKind, Report,
			StateStore, orchestrator,
		},
	},
};

#[test]
fn loop_guardrail_stops_repeated_validation_failures_after_three_observations() {
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("In Progress", &[]);
	let error = || {
		Report::new(RepoGateFailure::new(
			RepoGateFailureKind::VerifyCommandFailed,
			String::from("Repo verify command `cargo make test` failed: same assertion failed"),
		))
	};

	for attempt_number in 1..=2 {
		let issue_run = runtime_failure::loop_guardrail_issue_run(&config, &issue, attempt_number);

		assert!(
			orchestrator::retryable_failure_loop_guardrail_stop(
				&config,
				&state_store,
				&issue_run,
				&error(),
			)
			.expect("guardrail observation should persist")
			.is_none(),
			"guardrail should allow repair attempt {attempt_number}"
		);
	}

	let issue_run = runtime_failure::loop_guardrail_issue_run(&config, &issue, 3);
	let stop = orchestrator::retryable_failure_loop_guardrail_stop(
		&config,
		&state_store,
		&issue_run,
		&error(),
	)
	.expect("third matching failure should evaluate")
	.expect("third matching validation failure should stop");

	assert_eq!(stop.reason, orchestrator::LoopGuardrailReason::ValidationRepeat);
	assert_eq!(stop.consecutive_count, 3);
	assert_eq!(stop.source_error_class.as_deref(), Some("repo_gate_verify_failed"));

	let checkpoint = state_store
		.loop_guardrail_checkpoint(config.service_id(), &issue.id, "validation_repeat")
		.expect("validation checkpoint should read")
		.expect("validation checkpoint should exist");

	assert_eq!(checkpoint.consecutive_count(), 3);

	let events = state_store
		.list_private_execution_events(config.service_id(), &issue.id, &issue_run.run_id, 3)
		.expect("private events should list");

	assert_eq!(events.len(), 1);
	assert_eq!(events[0].event_type(), "loop_guardrail_checkpoint");
	assert_eq!(events[0].payload()["reason"], "validation_repeat");
}

#[test]
fn loop_guardrail_starts_architecture_recovery_when_boundary_is_within_authority() {
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("In Progress", &[]);
	let error = || {
		Report::new(RepoGateFailure::new(
			RepoGateFailureKind::VerifyCommandFailed,
			String::from("Repo verify command `cargo make test` failed: same assertion failed"),
		))
	};

	for attempt_number in 1..=2 {
		let issue_run = runtime_failure::loop_guardrail_issue_run(&config, &issue, attempt_number);

		assert!(
			orchestrator::retryable_failure_loop_guardrail_stop(
				&config,
				&state_store,
				&issue_run,
				&error(),
			)
			.expect("guardrail observation should persist")
			.is_none()
		);
	}

	let issue_run = runtime_failure::loop_guardrail_issue_run(&config, &issue, 3);
	let error = error();
	let stop = orchestrator::retryable_failure_loop_guardrail_stop(
		&config,
		&state_store,
		&issue_run,
		&error,
	)
	.expect("third matching failure should evaluate")
	.expect("third matching validation failure should stop");
	let decision = orchestrator::loop_guardrail_architecture_recovery_decision(
		&config,
		&state_store,
		&issue_run,
		stop,
		&error,
	)
	.expect("architecture recovery decision should record");
	let recovery = match decision {
		orchestrator::LoopGuardrailRecoveryDecision::Start(recovery) => recovery,
		orchestrator::LoopGuardrailRecoveryDecision::HumanRequired(_) => {
			panic!("repo-gate validation repeat should recover autonomously")
		},
	};

	assert_eq!(recovery.attempt_number, 1);
	assert!(recovery.detail.contains("materially different implementation strategy"));
	assert!(
		state_store
			.loop_guardrail_checkpoint(config.service_id(), &issue.id, "validation_repeat")
			.expect("checkpoint read should succeed")
			.is_none(),
		"started recovery should clear the stopped guardrail reason"
	);

	let events = state_store
		.list_private_execution_events(config.service_id(), &issue.id, &issue_run.run_id, 3)
		.expect("private events should list");

	assert!(events.iter().any(|event| {
		event.event_type() == "authority_boundary_check"
			&& event.payload()["disposition"] == "within_authority"
	}));
	assert!(events.iter().any(|event| {
		event.event_type() == "architecture_recovery_packet"
			&& event.payload()["reason_code"] == "architecture_recovery_started"
			&& event.payload()["authority_boundary_check"]["disposition"] == "within_authority"
			&& event.payload()["retained_worktree"]["tracked_status"].is_string()
			&& event.payload()["validation_failures"]["source_error_class"]
				== "repo_gate_verify_failed"
	}));
	assert!(events.iter().any(|event| {
		event.event_type() == "architecture_recovery_started"
			&& event.payload()["next_strategy"] == "materially_different_architecture_recovery"
	}));
}

#[test]
fn loop_guardrail_review_churn_blocks_landing_but_continues_recovery() {
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("In Progress", &[]);
	let issue_run = runtime_failure::loop_guardrail_issue_run(&config, &issue, 1);

	state_store
		.append_private_execution_event(
			config.service_id(),
			&issue.id,
			&issue_run.run_id,
			1,
			"review_checkpoint",
			serde_json::json!({
				"phase": "handoff",
				"status": "findings",
				"head_sha": "review-head",
				"nonclean_rounds": 3,
				"review": {
					"accepted_findings": [{
						"summary": "Accepted reviewer finding that exhausted review churn."
					}],
					"rejected_findings": [{
						"summary": "Rejected non-current reviewer comment."
					}],
					"finding_route_summary": {
						"route_counts": [
							{"route": "current_blocker", "count": 1},
							{"route": "risk_note", "count": 1}
						],
						"next_action": "Stop repair churn and run architecture recovery."
					}
				}
			}),
		)
		.expect("review checkpoint evidence should record");

	let stop = LoopGuardrailStopRequested {
		issue_identifier: issue.identifier.clone(),
		run_id: issue_run.run_id.clone(),
		reason: LoopGuardrailReason::ReviewChurn,
		consecutive_count: 3,
		fingerprint: String::from("review-head:3"),
		source_error_class: Some(String::from("review_policy_exhausted")),
		architecture_recovery_reason_code: None,
	};
	let error = Report::new(LoopGuardrailStopRequested {
		issue_identifier: issue.identifier.clone(),
		run_id: issue_run.run_id.clone(),
		reason: LoopGuardrailReason::ReviewChurn,
		consecutive_count: 3,
		fingerprint: String::from("review-head:3"),
		source_error_class: Some(String::from("review_policy_exhausted")),
		architecture_recovery_reason_code: None,
	});
	let decision = orchestrator::loop_guardrail_architecture_recovery_decision(
		&config,
		&state_store,
		&issue_run,
		stop,
		&error,
	)
	.expect("architecture recovery decision should record");

	assert!(
		matches!(decision, orchestrator::LoopGuardrailRecoveryDecision::Start(_)),
		"review-policy churn should continue recovery while blocking landing"
	);

	let events = state_store
		.list_private_execution_events(config.service_id(), &issue.id, &issue_run.run_id, 1)
		.expect("private events should list");

	assert!(events.iter().any(|event| {
		event.event_type() == "authority_boundary_check"
			&& event.payload()["policy_decision"] == "block_landing"
			&& event.payload()["policy"]["blocks_landing"] == true
			&& event.payload()["changed_surfaces"][0]["surface"] == "review_policy"
	}));
	assert!(events.iter().any(|event| {
		event.event_type() == "architecture_recovery_packet"
			&& event.payload()["reason_code"] == "architecture_recovery_started"
			&& event.payload()["authority_boundary_check"]["policy_decision"] == "block_landing"
			&& event.payload()["authority_boundary_check"]["blocks_landing"] == true
			&& event.payload()["review_findings"]["route_counts"][0]["route"] == "current_blocker"
			&& event.payload()["review_findings"]["route_counts"][0]["count"] == 1
			&& event.payload()["review_findings"]["route_counts"][1]["route"] == "risk_note"
			&& event.payload()["review_findings"]["route_next_action"]
				== "Stop repair churn and run architecture recovery."
	}));
}
