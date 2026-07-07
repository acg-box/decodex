use crate::orchestrator::harness_improvement::{
	HarnessImprovementCandidateSummary, HarnessOutcomeKind, HarnessOutcomeSignals,
	HarnessPhaseGoalOutcome, PrivateExecutionEvent, Value, candidates,
};

pub(in crate::orchestrator::harness_improvement) fn harness_outcome_signals(
	events: &[PrivateExecutionEvent],
	_outcome: HarnessOutcomeKind,
	error_class: Option<&str>,
) -> HarnessOutcomeSignals {
	let mut signals = HarnessOutcomeSignals::default();

	if let Some(error_class) =
		error_class.filter(|class| harness_error_class_is_validation_failure(class))
	{
		signals.validation_failure_count += 1;

		signals.validation_failure_classes.insert(error_class.to_owned());
	}

	for event in events {
		match event.event_type() {
			"phase_goal_completed" | "phase_goal_set" => {
				push_phase_goal_signal(&mut signals, event)
			},
			"review_checkpoint" => push_review_signal(&mut signals, event.payload()),
			"loop_guardrail_checkpoint" => push_guardrail_signal(&mut signals, event.payload()),
			"authority_boundary_check" => {
				push_authority_boundary_signal(&mut signals, event.payload())
			},
			"architecture_recovery_terminal" => {
				push_architecture_recovery_signal(&mut signals, event.payload());
			},
			"progress_checkpoint" => push_progress_signal(&mut signals, event.payload()),
			_ => {},
		}
	}

	signals
}

fn harness_error_class_is_validation_failure(error_class: &str) -> bool {
	error_class.starts_with("repo_gate_")
		|| matches!(error_class, "validation_repeat" | "validation_failure_repeated")
}

fn push_phase_goal_signal(signals: &mut HarnessOutcomeSignals, event: &PrivateExecutionEvent) {
	let payload = event.payload();
	let nested = payload.get("payload").unwrap_or(payload);
	let signal = candidates::json_string(nested.get("signal"))
		.or_else(|| candidates::json_string(payload.get("signal")));
	let phase = candidates::json_string(nested.get("phase"))
		.or_else(|| candidates::json_string(payload.get("phase")));
	let status = candidates::json_string(nested.get("status"));

	if signal.as_deref() == Some("validation_fail") {
		signals.validation_failure_count += 1;

		signals.validation_failure_classes.insert(String::from("phase_goal_validation_fail"));
	}
	if phase.as_deref().is_some_and(|phase| phase.contains("repair")) {
		signals.repair_phase_events += 1;
	}

	signals.phase_goals.push(HarnessPhaseGoalOutcome {
		event_type: event.event_type().to_owned(),
		phase,
		signal,
		status,
	});
}

fn push_review_signal(signals: &mut HarnessOutcomeSignals, payload: &Value) {
	if let Some(status) = candidates::json_string(payload.get("status")) {
		signals.review_statuses.insert(status);
	}

	signals.nonclean_rounds = signals
		.nonclean_rounds
		.max(payload.get("nonclean_rounds").and_then(Value::as_i64).unwrap_or(0));

	let review = payload.get("review").unwrap_or(payload);

	signals.accepted_finding_count += candidates::json_array_len(review.get("accepted_findings"));
	signals.rejected_finding_count += candidates::json_array_len(review.get("rejected_findings"));
}

fn push_guardrail_signal(signals: &mut HarnessOutcomeSignals, payload: &Value) {
	if let Some(reason) = candidates::json_string(payload.get("reason")) {
		signals.guardrail_reasons.insert(reason);
	}
	if let Some(error_class) = candidates::json_string(payload.get("source_error_class"))
		&& harness_error_class_is_validation_failure(&error_class)
	{
		signals.validation_failure_count += 1;

		signals.validation_failure_classes.insert(error_class);
	}
}

fn push_authority_boundary_signal(signals: &mut HarnessOutcomeSignals, payload: &Value) {
	if let Some(disposition) = candidates::json_string(payload.get("disposition")) {
		signals.authority_boundary_dispositions.insert(disposition.clone());

		if disposition != "within_authority" {
			signals.authority_boundary_failed_check_count += 1;
		}
		if matches!(disposition.as_str(), "requires_human" | "insufficient_evidence")
			&& candidates::json_array_len(payload.get("improvement_signals")) == 0
			&& candidates::authority_boundary_final_reason_mentions_underspecified(payload)
		{
			let target = candidates::first_decision_contract_target(payload)
				.unwrap_or_else(|| String::from("issue:local-readback"));

			signals.authority_boundary_candidates.push(HarnessImprovementCandidateSummary {
				kind: String::from("underspecified_decision_contract"),
				reason_code: String::from("authority_underspecified"),
				target,
				source_event_count: 1,
				recommendation: String::from(
					"Add explicit authority-envelope fields before retrying autonomous recovery.",
				),
			});
		}
	}
	if let Some(improvement_signals) = payload.get("improvement_signals").and_then(Value::as_array)
	{
		for signal in improvement_signals {
			let Some(kind) = candidates::json_string(signal.get("kind")) else {
				continue;
			};
			let Some(reason_code) = candidates::json_string(signal.get("reason_code")) else {
				continue;
			};
			let Some(target) = candidates::json_string(signal.get("target")) else {
				continue;
			};
			let Some(recommendation) = candidates::json_string(signal.get("recommendation")) else {
				continue;
			};

			signals.authority_boundary_candidates.push(HarnessImprovementCandidateSummary {
				kind,
				reason_code,
				target,
				source_event_count: 1,
				recommendation,
			});
		}
	}
}

fn push_architecture_recovery_signal(signals: &mut HarnessOutcomeSignals, payload: &Value) {
	if candidates::json_string(payload.get("reason_code")).as_deref()
		== Some("architecture_recovery_exhausted")
	{
		signals.architecture_recovery_budget_exhausted_count += 1;
	}
}

fn push_progress_signal(signals: &mut HarnessOutcomeSignals, payload: &Value) {
	if candidates::json_string(payload.get("phase")).is_some_and(|phase| phase.contains("repair")) {
		signals.repair_phase_events += 1;
	}
}
