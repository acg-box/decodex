use crate::orchestrator::{self, ProtocolActivityEventSummary, ProtocolActivitySummary};

pub(crate) fn render_protocol_activity_summary(
	summary: Option<&ProtocolActivitySummary>,
) -> String {
	let Some(summary) = summary else {
		return String::from("none");
	};
	let turn = summary.turn_status.as_deref().unwrap_or("none");
	let wait = summary.waiting_reason.as_deref().unwrap_or("none");
	let rate_limit = summary.rate_limit_status.as_deref().unwrap_or("none");
	let recent = if summary.recent_events.is_empty() {
		String::from("none")
	} else {
		summary
			.recent_events
			.iter()
			.rev()
			.take(5)
			.map(render_protocol_activity_event_summary)
			.collect::<Vec<_>>()
			.join(", ")
	};

	format!("turn={turn}; waiting={wait}; rate_limit={rate_limit}; recent={recent}")
}

fn render_protocol_activity_event_summary(event: &ProtocolActivityEventSummary) -> String {
	event.detail.as_ref().map_or_else(
		|| event.event_type.clone(),
		|detail| format!("{}:{}", event.event_type, render_protocol_activity_detail(detail)),
	)
}

fn render_protocol_activity_detail(detail: &str) -> &str {
	if orchestrator::operator_protocol_activity_detail_is_public(detail) {
		detail
	} else {
		"redacted_sensitive_detail"
	}
}
