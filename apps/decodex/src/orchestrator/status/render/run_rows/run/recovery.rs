use crate::orchestrator::{OperatorContinuationRecoveryStatus, OperatorPhaseAcceptanceStatus};

pub(in crate::orchestrator::status::render::run_rows::run) fn render_continuation_recovery_summary(
	recovery: Option<&OperatorContinuationRecoveryStatus>,
) -> String {
	let Some(recovery) = recovery else {
		return String::from("none");
	};
	let message = recovery
		.source_error_message
		.as_deref()
		.map(single_line_status_value)
		.unwrap_or_else(|| String::from("none"));

	format!(
		"state={} source_phase={} next_phase={} source_error_class={} source_error_message={} count={}/{} budget_exceeded={} recorded_at={} run_id={} attempt={} next_action={}",
		recovery.state,
		recovery.source_phase,
		recovery.next_phase,
		recovery.source_error_class,
		message,
		recovery.recovery_count,
		recovery.automatic_continuation_limit,
		if recovery.budget_exceeded { "yes" } else { "no" },
		recovery.recorded_at,
		recovery.run_id,
		recovery.attempt_number,
		recovery.next_action,
	)
}

pub(in crate::orchestrator::status::render::run_rows::run) fn render_phase_acceptance_summary(
	acceptance: Option<&OperatorPhaseAcceptanceStatus>,
) -> String {
	let Some(acceptance) = acceptance else {
		return String::from("none");
	};
	let surfaces = if acceptance.changed_surfaces.is_empty() {
		String::from("none")
	} else {
		acceptance.changed_surfaces.join(",")
	};

	format!(
		"phase={} decision={} reason={} objective_covered={} effective_delta={} surfaces={} non_goal_passed={} validation_passed={} recorded_at={} run_id={} attempt={} next_action={}",
		acceptance.phase,
		acceptance.decision,
		acceptance.reason_code,
		if acceptance.objective_covered { "yes" } else { "no" },
		if acceptance.effective_delta_present { "yes" } else { "no" },
		surfaces,
		if acceptance.non_goal_passed { "yes" } else { "no" },
		if acceptance.validation_passed { "yes" } else { "no" },
		acceptance.recorded_at,
		acceptance.run_id,
		acceptance.attempt_number,
		acceptance.next_action
	)
}

fn single_line_status_value(value: &str) -> String {
	value.split_whitespace().collect::<Vec<_>>().join(" ")
}
