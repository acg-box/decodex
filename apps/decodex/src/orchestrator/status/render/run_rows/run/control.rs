use crate::orchestrator::{self, EXECUTION_LIVENESS_PROCESS_IDENTITY_MISMATCH, OperatorRunStatus};

pub(in crate::orchestrator::status::render::run_rows::run) fn render_lane_control_conditions(
	run: &OperatorRunStatus,
) -> String {
	if run.lane_control_conditions.is_empty() {
		String::from("none")
	} else {
		run.lane_control_conditions.join(",")
	}
}

pub(in crate::orchestrator::status::render::run_rows::run) fn render_optional_bool(
	value: Option<bool>,
) -> String {
	value.map_or_else(|| String::from("none"), |value| if value { "yes" } else { "no" }.into())
}

pub(in crate::orchestrator::status::render::run_rows::run) fn operator_run_queue_lease_summary(
	run: &OperatorRunStatus,
) -> String {
	if run.run_lease {
		return String::from("held");
	}

	match run.execution_liveness.as_str() {
		"process_alive" => String::from("not_held (process_alive keeps lane visible)"),
		"thread_active" => String::from("not_held (thread_active keeps lane visible)"),
		"protocol_observed" => String::from("not_held (protocol_observed keeps lane visible)"),
		"process_stopped" => String::from("not_held (process_stopped needs attention)"),
		EXECUTION_LIVENESS_PROCESS_IDENTITY_MISMATCH
			if orchestrator::operator_run_has_recent_app_server_execution(run) =>
			String::from("not_held (app_server_activity keeps lane visible)"),
		EXECUTION_LIVENESS_PROCESS_IDENTITY_MISMATCH =>
			String::from("not_held (process_identity_mismatch needs attention)"),
		_ => String::from("not_held"),
	}
}
