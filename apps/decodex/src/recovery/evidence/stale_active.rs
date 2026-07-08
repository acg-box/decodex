mod control;
mod guardrail;
mod harness;
mod release_audit;
mod runtime;

use crate::{
	recovery::process_liveness::StaleActiveProcessLiveness,
	state::{PrivateExecutionEvent, ProjectRunStatus},
};

pub(in crate::recovery) fn stale_active_private_event_allows_release(
	event: &PrivateExecutionEvent,
	marker_liveness: StaleActiveProcessLiveness,
	release_audit_present: bool,
) -> bool {
	release_audit::stale_active_private_event_is_release_audit(event)
		|| control::stale_active_private_event_is_failed_control_attempt(event)
		|| ((marker_liveness == StaleActiveProcessLiveness::NotAlive || release_audit_present)
			&& control::stale_active_event_is_dead_process_telemetry(event))
		|| runtime::stale_active_private_event_is_stale_runtime_marker(event)
		|| runtime::stale_active_private_event_is_probing_checkpoint(event)
		|| guardrail::stale_active_private_event_is_no_diff_guardrail(event)
		|| runtime::stale_active_event_is_phase_goal_failure_telemetry(event)
		|| harness::stale_active_event_is_no_progress_harness(event)
}

pub(in crate::recovery) fn stale_active_private_event_is_release_audit_for_run(
	event: &PrivateExecutionEvent,
	run: Option<&ProjectRunStatus>,
) -> bool {
	let Some(run) = run else {
		return false;
	};

	release_audit::stale_active_private_event_is_release_audit(event)
		&& event.run_id() == run.run_id()
		&& event.attempt_number() == run.attempt_number()
}
