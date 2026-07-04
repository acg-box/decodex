use serde_json::Value;

use crate::orchestrator::operator_http::{
	DashboardClientMessage, DashboardControlAck, DashboardWebSocketSession, StateStore,
	dashboard::{control_ack, control_actions, subscription},
	types::DashboardClientSubscription,
};

pub(crate) fn dashboard_control_ack_should_push_snapshot(ack: &Value) -> bool {
	control_ack::dashboard_control_ack_should_push_snapshot(ack)
}

pub(crate) fn dashboard_control_ack_should_push_run_activity(ack: &Value) -> bool {
	control_ack::dashboard_control_ack_should_push_run_activity(ack)
}

pub(crate) fn dashboard_control_ready_payload(subscription: &DashboardClientSubscription) -> Value {
	control_ack::dashboard_control_ready_payload(subscription)
}

pub(crate) fn handle_dashboard_client_message(
	session: &mut DashboardWebSocketSession,
	state_store: &StateStore,
	payload: &[u8],
) -> Value {
	let message = match serde_json::from_slice::<DashboardClientMessage>(payload) {
		Ok(message) => message,
		Err(error) => {
			return control_ack::dashboard_control_ack_value(DashboardControlAck {
				request_id: None,
				action: "parse",
				accepted: false,
				status: "invalid_message",
				message: &format!("Dashboard control message was not valid JSON: {error}"),
				project_id: None,
				issue_id: None,
				run_id: None,
				subscription: Some(&session.subscription),
			});
		},
	};
	let action = message
		.action
		.as_deref()
		.map(str::trim)
		.filter(|value| !value.is_empty())
		.unwrap_or(message.message_type.as_str())
		.to_owned();

	match message.message_type.as_str() {
		"subscribe" => {
			session.subscription = subscription::dashboard_subscription_from_message(&message);

			control_ack::dashboard_control_ack_for_message(
				session,
				&message,
				"subscribe",
				true,
				"subscribed",
				"Dashboard stream subscription updated.",
			)
		},
		"control" => control_actions::handle_dashboard_control_action(
			session,
			state_store,
			&message,
			&action,
		),
		_ => control_ack::dashboard_control_ack_for_message(
			session,
			&message,
			&action,
			false,
			"unsupported_message",
			"Unsupported dashboard WebSocket message type.",
		),
	}
}
