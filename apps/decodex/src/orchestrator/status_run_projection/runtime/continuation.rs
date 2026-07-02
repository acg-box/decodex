use crate::orchestrator::{
	OperatorContinuationRecoveryStatus, PHASE_GOAL_RECOVERY_AUTOMATIC_CONTINUATION_LIMIT,
	PHASE_GOAL_RECOVERY_BLOCKED_EVENT_TYPE, PHASE_GOAL_RECOVERY_EVENT_TYPE, PrivateExecutionEvent,
	ProjectLoopEvidenceSnapshot, ProjectRunStatus, Value,
};

pub(in crate::orchestrator) fn operator_run_continuation_recovery_status(
	loop_evidence: &ProjectLoopEvidenceSnapshot,
	run: &ProjectRunStatus,
) -> Option<OperatorContinuationRecoveryStatus> {
	let recovery_events = loop_evidence
		.private_events_for_issue(run.issue_id())
		.into_iter()
		.filter(|event| event.attempt_number() <= run.attempt_number())
		.filter_map(operator_continuation_recovery_event_status)
		.collect::<Vec<_>>();
	let latest = recovery_events.last()?.clone();
	let recovery_count = recovery_events
		.iter()
		.filter(|event| {
			event.source_phase == latest.source_phase
				&& event.source_error_class == latest.source_error_class
				&& event.state == "continuation_scheduled"
		})
		.count() as i64;
	let budget_exceeded = latest.state == "continuation_blocked"
		|| recovery_count > PHASE_GOAL_RECOVERY_AUTOMATIC_CONTINUATION_LIMIT;

	Some(OperatorContinuationRecoveryStatus {
		state: latest.state,
		source_phase: latest.source_phase,
		next_phase: latest.next_phase,
		source_error_class: latest.source_error_class,
		source_error_message: latest.source_error_message,
		recorded_at: latest.recorded_at,
		run_id: latest.run_id,
		attempt_number: latest.attempt_number,
		recovery_count,
		automatic_continuation_limit: PHASE_GOAL_RECOVERY_AUTOMATIC_CONTINUATION_LIMIT,
		budget_exceeded,
		next_action: if budget_exceeded {
			String::from("stop_auto_continuation_and_request_architecture_recovery")
		} else {
			String::from("monitor_continuation_recovery")
		},
	})
}

pub(in crate::orchestrator) fn operator_continuation_recovery_event_status(
	event: &PrivateExecutionEvent,
) -> Option<OperatorContinuationRecoveryStatus> {
	let state = match event.event_type() {
		PHASE_GOAL_RECOVERY_EVENT_TYPE => "continuation_scheduled",
		PHASE_GOAL_RECOVERY_BLOCKED_EVENT_TYPE => "continuation_blocked",
		_ => return None,
	};
	let payload = event.payload();
	let event_payload = payload.get("payload").unwrap_or(payload);
	let source_phase = payload
		.get("phase")
		.and_then(Value::as_str)
		.or_else(|| event_payload.get("sourcePhase").and_then(Value::as_str))?
		.to_owned();
	let next_phase = event_payload.get("nextPhase")?.as_str()?.to_owned();
	let source_error_class = event_payload.get("sourceErrorClass")?.as_str()?.to_owned();
	let source_error_message =
		event_payload.get("sourceErrorMessage").and_then(Value::as_str).map(str::to_owned);

	Some(OperatorContinuationRecoveryStatus {
		state: String::from(state),
		source_phase,
		next_phase,
		source_error_class,
		source_error_message,
		recorded_at: event.recorded_at().to_owned(),
		run_id: event.run_id().to_owned(),
		attempt_number: event.attempt_number(),
		recovery_count: 0,
		automatic_continuation_limit: PHASE_GOAL_RECOVERY_AUTOMATIC_CONTINUATION_LIMIT,
		budget_exceeded: false,
		next_action: String::new(),
	})
}
