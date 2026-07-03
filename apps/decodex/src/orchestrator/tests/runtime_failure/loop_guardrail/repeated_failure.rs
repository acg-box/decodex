use crate::orchestrator::{
	RepoGateFailure,
	tests::{
		self,
		runtime_failure::{self, RepoGateFailureKind, Report, StateStore, orchestrator},
	},
};

#[test]
fn loop_guardrail_requires_human_when_boundary_evidence_is_missing() {
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("In Progress", &[]);

	for attempt_number in 1..=2 {
		let issue_run = runtime_failure::loop_guardrail_issue_run(&config, &issue, attempt_number);
		let error = Report::msg("child exited without useful change");

		assert!(
			orchestrator::retryable_failure_loop_guardrail_stop(
				&config,
				&state_store,
				&issue_run,
				&error,
			)
			.expect("guardrail observation should persist")
			.is_none()
		);
	}

	let issue_run = runtime_failure::loop_guardrail_issue_run(&config, &issue, 3);
	let error = Report::msg("child exited without useful change");
	let stop = orchestrator::retryable_failure_loop_guardrail_stop(
		&config,
		&state_store,
		&issue_run,
		&error,
	)
	.expect("third no-diff observation should evaluate")
	.expect("no effective diff should stop");
	let decision = orchestrator::loop_guardrail_architecture_recovery_decision(
		&config,
		&state_store,
		&issue_run,
		stop,
		&error,
	)
	.expect("architecture recovery decision should record");
	let terminal_stop = match decision {
		orchestrator::LoopGuardrailRecoveryDecision::Start(_) => {
			panic!("missing authority evidence must not start recovery")
		},
		orchestrator::LoopGuardrailRecoveryDecision::HumanRequired(stop) => stop,
	};

	assert_eq!(terminal_stop.terminal_error_class(), "contract_boundary_required");

	let events = state_store
		.list_private_execution_events(config.service_id(), &issue.id, &issue_run.run_id, 3)
		.expect("private events should list");

	assert!(events.iter().any(|event| {
		event.event_type() == "authority_boundary_check"
			&& event.payload()["policy_decision"] == "requires_human_decision"
			&& event.payload()["disposition"] == "requires_human"
	}));
	assert!(events.iter().any(|event| {
		event.event_type() == "architecture_recovery_packet"
			&& event.payload()["reason_code"] == "contract_boundary_required"
			&& event.payload()["authority_boundary_check"]["policy_decision"]
				== "requires_human_decision"
	}));
	assert!(events.iter().any(|event| {
		event.event_type() == "architecture_recovery_terminal"
			&& event.payload()["reason_code"] == "contract_boundary_required"
	}));
	assert!(events.iter().any(|event| {
		event.event_type() == "authority_decision_request"
			&& event.payload()["reason"] == "contract_boundary_required"
	}));
}

#[test]
fn loop_guardrail_stops_validation_repeat_when_validation_text_changes() {
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("In Progress", &[]);

	for attempt_number in 1..=2 {
		let issue_run = runtime_failure::loop_guardrail_issue_run(&config, &issue, attempt_number);
		let error = Report::new(RepoGateFailure::new(
			RepoGateFailureKind::VerifyCommandFailed,
			format!("Repo verify command failed with assertion variant {attempt_number}"),
		));

		assert!(
			orchestrator::retryable_failure_loop_guardrail_stop(
				&config,
				&state_store,
				&issue_run,
				&error,
			)
			.expect("guardrail observation should persist")
			.is_none()
		);
	}

	let issue_run = runtime_failure::loop_guardrail_issue_run(&config, &issue, 3);
	let error = Report::new(RepoGateFailure::new(
		RepoGateFailureKind::VerifyCommandFailed,
		String::from("Repo verify command failed with assertion variant 3"),
	));
	let stop = orchestrator::retryable_failure_loop_guardrail_stop(
		&config,
		&state_store,
		&issue_run,
		&error,
	)
	.expect("third unchanged delta should evaluate")
	.expect("normalized validation repeat should stop");

	assert_eq!(stop.reason, orchestrator::LoopGuardrailReason::ValidationRepeat);
	assert_eq!(stop.consecutive_count, 3);
	assert_eq!(stop.source_error_class.as_deref(), Some("repo_gate_verify_failed"));

	let validation_checkpoint = state_store
		.loop_guardrail_checkpoint(config.service_id(), &issue.id, "validation_repeat")
		.expect("validation checkpoint should read")
		.expect("validation checkpoint should exist");

	assert_eq!(validation_checkpoint.consecutive_count(), 3);
	assert!(
		validation_checkpoint
			.fingerprint()
			.contains("repo_gate_verify_failed:repo_gate:validation_repair"),
		"normalized fingerprint should not include raw error text"
	);
}

#[test]
fn loop_guardrail_stops_no_effective_diff_for_retryable_errors_without_delta() {
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("In Progress", &[]);

	for attempt_number in 1..=2 {
		let issue_run = runtime_failure::loop_guardrail_issue_run(&config, &issue, attempt_number);
		let error = Report::msg("child exited without useful change");

		assert!(
			orchestrator::retryable_failure_loop_guardrail_stop(
				&config,
				&state_store,
				&issue_run,
				&error,
			)
			.expect("guardrail observation should persist")
			.is_none()
		);
	}

	let issue_run = runtime_failure::loop_guardrail_issue_run(&config, &issue, 3);
	let error = Report::msg("child exited without useful change");
	let stop = orchestrator::retryable_failure_loop_guardrail_stop(
		&config,
		&state_store,
		&issue_run,
		&error,
	)
	.expect("third no-diff observation should evaluate")
	.expect("no effective diff should stop");

	assert_eq!(stop.reason, orchestrator::LoopGuardrailReason::NoEffectiveDiff);
	assert_eq!(stop.consecutive_count, 3);
	assert_eq!(stop.source_error_class, None);

	let checkpoint = state_store
		.loop_guardrail_checkpoint(config.service_id(), &issue.id, "no_effective_diff")
		.expect("no-diff checkpoint should read")
		.expect("no-diff checkpoint should exist");

	assert_eq!(checkpoint.consecutive_count(), 3);
}

#[test]
fn loop_guardrail_does_not_classify_committed_branch_delta_as_no_effective_diff() {
	let (temp_dir, config, _workflow) = tests::temp_project_layout();
	let remote_root = temp_dir.path().join("origin.git");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("In Progress", &[]);

	runtime_failure::add_origin_remote(config.repo_root(), &remote_root);
	runtime_failure::checkout_new_branch(config.repo_root(), "x/pubfi-pub-101");
	runtime_failure::commit_worktree_change(
		config.repo_root(),
		"ready.txt",
		"implementation complete\n",
		"implement issue scope",
	);

	assert_eq!(runtime_failure::git_output(config.repo_root(), &["status", "--porcelain"]), "");

	for attempt_number in 1..=3 {
		let issue_run = runtime_failure::loop_guardrail_issue_run(&config, &issue, attempt_number);
		let error = Report::msg("child exited after committed issue-scoped work");
		let stop = orchestrator::retryable_failure_loop_guardrail_stop(
			&config,
			&state_store,
			&issue_run,
			&error,
		)
		.expect("guardrail observation should evaluate");

		assert!(stop.is_none(), "committed branch delta must not be reported as no_effective_diff");
	}

	assert!(
		state_store
			.loop_guardrail_checkpoint(config.service_id(), &issue.id, "no_effective_diff")
			.expect("checkpoint lookup should succeed")
			.is_none()
	);
}
