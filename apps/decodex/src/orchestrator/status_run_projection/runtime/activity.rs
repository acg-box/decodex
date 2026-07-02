use crate::orchestrator::{
	ChildAgentActivitySummary, OperatorRunAppServerState, ProtocolActivitySummary,
	RUN_LEASE_IDLE_TIMEOUT, RunActivityMarker, status_run_projection,
};
use crate::tracker::public_text;

pub(in crate::orchestrator) fn operator_run_child_agent_activity(
	marker: Option<&RunActivityMarker>,
	stored_summary: Option<&ChildAgentActivitySummary>,
	now_unix_epoch: i64,
) -> Option<ChildAgentActivitySummary> {
	if let Some(marker) = marker
		&& let Some(summary) = marker.child_agent_activity()
	{
		return Some(summary.clone().live_projection(now_unix_epoch));
	}

	stored_summary.cloned().map(ChildAgentActivitySummary::sealed_durable)
}

pub(in crate::orchestrator) fn operator_run_protocol_activity(
	marker: Option<&RunActivityMarker>,
	stored_summary: Option<&ProtocolActivitySummary>,
	app_server_state: &OperatorRunAppServerState,
	child_agent_activity: Option<&ChildAgentActivitySummary>,
	protocol_idle_for_seconds: Option<i64>,
	is_running: bool,
) -> Option<ProtocolActivitySummary> {
	let mut summary = marker
		.and_then(RunActivityMarker::protocol_activity)
		.or(stored_summary)
		.cloned()
		.unwrap_or_default();

	if is_running && summary.waiting_reason.is_none() && app_server_state.interactive_requested {
		summary.waiting_reason = Some(String::from("approval_or_user_input"));
	}
	if is_running
		&& summary.waiting_reason.is_none()
		&& let Some(child_agent_activity) = child_agent_activity
		&& let Some(current_bucket) = child_agent_activity.current_bucket.as_deref()
	{
		summary.waiting_reason =
			Some(status_run_projection::protocol_wait_reason_from_child_bucket(current_bucket));
	}
	if is_running
		&& summary.waiting_reason.is_none()
		&& protocol_idle_for_seconds.is_some_and(|idle_for| {
			u64::try_from(idle_for)
				.is_ok_and(|idle_for| idle_for < RUN_LEASE_IDLE_TIMEOUT.as_secs())
		}) {
		summary.waiting_reason = Some(String::from("protocol_idleness"));
	}
	if summary.turn_status.is_none()
		&& summary.waiting_reason.is_none()
		&& summary.rate_limit_status.is_none()
		&& summary.recent_events.is_empty()
	{
		return None;
	}

	sanitize_operator_protocol_activity_summary(&mut summary);

	Some(summary)
}

pub(in crate::orchestrator) fn sanitize_operator_protocol_activity_summary(
	summary: &mut ProtocolActivitySummary,
) {
	for event in &mut summary.recent_events {
		if let Some(detail) = event.detail.as_deref()
			&& !operator_protocol_activity_detail_is_public(detail)
		{
			event.detail = Some(String::from("redacted_sensitive_detail"));
		}
	}
}

pub(in crate::orchestrator) fn operator_protocol_activity_detail_is_public(detail: &str) -> bool {
	public_text::validate_public_text_field("protocol_activity.detail", detail).is_ok()
		&& !contains_protocol_activity_host_path_shape(detail)
		&& !contains_protocol_activity_secret_shape(detail)
}

pub(in crate::orchestrator) fn contains_protocol_activity_host_path_shape(detail: &str) -> bool {
	let mut previous = None;
	let mut chars = detail.char_indices().peekable();

	while let Some((index, character)) = chars.next() {
		if character != '/' {
			previous = Some(character);

			continue;
		}
		if previous == Some(':') || previous == Some('/') {
			previous = Some(character);

			continue;
		}

		let path_boundary = index == 0
			|| previous.is_some_and(|previous| {
				previous.is_whitespace()
					|| matches!(previous, '"' | '\'' | '`' | '(' | '[' | '{' | '=')
			});
		let path_component = chars
			.peek()
			.map(|(_, next)| next.is_ascii_alphanumeric() || matches!(next, '.' | '_' | '-'))
			.unwrap_or(false);

		if path_boundary && path_component {
			return true;
		}

		previous = Some(character);
	}

	false
}

pub(in crate::orchestrator) fn contains_protocol_activity_secret_shape(detail: &str) -> bool {
	detail.split(protocol_activity_token_separator).any(|token| {
		let normalized = token.to_ascii_lowercase();

		normalized.starts_with("ghp_")
			|| normalized.starts_with("github_pat_")
			|| is_high_entropy_protocol_activity_token(token)
	})
}

pub(in crate::orchestrator) fn protocol_activity_token_separator(character: char) -> bool {
	!(character.is_ascii_alphanumeric() || character == '_' || character == '-')
}

pub(in crate::orchestrator) fn is_high_entropy_protocol_activity_token(token: &str) -> bool {
	if token.len() < 24 {
		return false;
	}

	let mut has_uppercase = false;
	let mut has_lowercase = false;
	let mut has_digit = false;
	let mut alphanumeric_count = 0;

	for character in token.chars() {
		if !character.is_ascii_alphanumeric() {
			continue;
		}

		alphanumeric_count += 1;
		has_uppercase |= character.is_ascii_uppercase();
		has_lowercase |= character.is_ascii_lowercase();
		has_digit |= character.is_ascii_digit();
	}

	alphanumeric_count >= 24 && has_uppercase && has_lowercase && has_digit
}
