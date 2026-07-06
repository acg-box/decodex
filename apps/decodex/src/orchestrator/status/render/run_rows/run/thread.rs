use crate::orchestrator::OperatorRunStatus;

pub(in crate::orchestrator::status::render::run_rows::run) fn render_run_protocol_event(
	run: &OperatorRunStatus,
) -> String {
	match (&run.last_event_type, &run.last_event_at) {
		(Some(event_type), Some(timestamp)) => format!("{event_type} @ {timestamp}"),
		(Some(event_type), None) => event_type.clone(),
		(None, Some(timestamp)) => timestamp.clone(),
		(None, None) => String::from("none"),
	}
}

pub(in crate::orchestrator::status::render::run_rows::run) fn render_run_thread_active_flags(
	run: &OperatorRunStatus,
) -> String {
	if run.thread_active_flags.is_empty() {
		String::from("none")
	} else {
		run.thread_active_flags.join(",")
	}
}
