use serde_json::Value;

use super::{
	super::{
		DashboardClientMessage, DashboardControlAck, DashboardWebSocketSession, StateStore,
		accounts, json, types::DashboardClientSubscription,
	},
	subscription::{
		dashboard_required_account_selector, dashboard_subscription_from_message,
		dashboard_subscription_payload,
	},
};

pub(super) fn dashboard_control_ack_should_push_snapshot(ack: &Value) -> bool {
	ack.get("accepted").and_then(Value::as_bool).unwrap_or(false)
		&& matches!(
			ack.get("action").and_then(Value::as_str),
			Some("selectAccount" | "clearAccountSelection")
		)
}

pub(super) fn dashboard_control_ack_should_push_run_activity(ack: &Value) -> bool {
	ack.get("accepted").and_then(Value::as_bool).unwrap_or(false)
		&& matches!(
			ack.get("action").and_then(Value::as_str),
			Some("subscribe" | "focus" | "clearFocus" | "selectAccount" | "clearAccountSelection")
		)
}

pub(super) fn dashboard_control_ready_payload(subscription: &DashboardClientSubscription) -> Value {
	json!({
		"supportedActions": [
			"subscribe",
			"focus",
			"clearFocus",
			"selectAccount",
			"clearAccountSelection",
			"ack"
		],
		"subscription": dashboard_subscription_payload(subscription),
	})
}

pub(super) fn handle_dashboard_client_message(
	session: &mut DashboardWebSocketSession,
	state_store: &StateStore,
	payload: &[u8],
) -> Value {
	let message = match serde_json::from_slice::<DashboardClientMessage>(payload) {
		Ok(message) => message,
		Err(error) => {
			return dashboard_control_ack_value(DashboardControlAck {
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
			session.subscription = dashboard_subscription_from_message(&message);

			dashboard_control_ack_for_message(
				session,
				&message,
				"subscribe",
				true,
				"subscribed",
				"Dashboard stream subscription updated.",
			)
		},
		"control" => handle_dashboard_control_action(session, state_store, &message, &action),
		_ => dashboard_control_ack_for_message(
			session,
			&message,
			&action,
			false,
			"unsupported_message",
			"Unsupported dashboard WebSocket message type.",
		),
	}
}

pub(super) fn handle_dashboard_control_action(
	session: &mut DashboardWebSocketSession,
	state_store: &StateStore,
	message: &DashboardClientMessage,
	action: &str,
) -> Value {
	match action {
		"focus" => dashboard_focus_control_ack(session, message, action),
		"clearFocus" | "clearSubscription" =>
			dashboard_clear_focus_control_ack(session, message, action),
		"selectAccount" =>
			dashboard_account_selection_control_ack(session, state_store, message, action, true),
		"clearAccountSelection" =>
			dashboard_account_selection_control_ack(session, state_store, message, action, false),
		"ack" | "ackNotice" => dashboard_control_ack_for_message(
			session,
			message,
			action,
			true,
			"acknowledged",
			"Dashboard acknowledgement recorded for this browser session only.",
		),
		_ => dashboard_unsupported_control_ack(session, message, action),
	}
}

pub(super) fn dashboard_focus_control_ack(
	session: &mut DashboardWebSocketSession,
	message: &DashboardClientMessage,
	action: &str,
) -> Value {
	session.subscription = dashboard_subscription_from_message(message);

	dashboard_control_ack_for_message(
		session,
		message,
		action,
		true,
		"focused",
		"Dashboard focus updated for this WebSocket session.",
	)
}

pub(super) fn dashboard_clear_focus_control_ack(
	session: &mut DashboardWebSocketSession,
	message: &DashboardClientMessage,
	action: &str,
) -> Value {
	session.subscription = DashboardClientSubscription::default();

	dashboard_control_ack_value(DashboardControlAck {
		request_id: message.request_id.as_deref(),
		action,
		accepted: true,
		status: "cleared",
		message: "Dashboard focus cleared for this WebSocket session.",
		project_id: None,
		issue_id: None,
		run_id: None,
		subscription: Some(&session.subscription),
	})
}

pub(super) fn dashboard_account_selection_control_ack(
	session: &DashboardWebSocketSession,
	_state_store: &StateStore,
	message: &DashboardClientMessage,
	action: &str,
	set_fixed: bool,
) -> Value {
	let selector = if set_fixed {
		match dashboard_required_account_selector(message) {
			Some(selector) => Some(selector),
			None => {
				return dashboard_control_ack_value(DashboardControlAck {
					request_id: message.request_id.as_deref(),
					action,
					accepted: false,
					status: "missing_account",
					message: "Account selection requires an account selector.",
					project_id: None,
					issue_id: message.issue_id.as_deref(),
					run_id: message.run_id.as_deref(),
					subscription: Some(&session.subscription),
				});
			},
		}
	} else {
		None
	};
	let result = if let Some(selector) = selector {
		accounts::account_select(selector).map(|_| ())
	} else {
		accounts::account_clear().map(|_| ())
	};
	let (accepted, status, copy) = match (set_fixed, result) {
		(true, Ok(())) => (
			true,
			"fixed",
			String::from("Global Codex account pool now pins new runs to the selected account."),
		),
		(false, Ok(())) => (
			true,
			"balanced",
			String::from("Global Codex account pool now uses balanced account selection."),
		),
		(_, Err(error)) => (false, "failed", error.to_string()),
	};

	dashboard_control_ack_value(DashboardControlAck {
		request_id: message.request_id.as_deref(),
		action,
		accepted,
		status,
		message: &copy,
		project_id: None,
		issue_id: message.issue_id.as_deref(),
		run_id: message.run_id.as_deref(),
		subscription: Some(&session.subscription),
	})
}

pub(super) fn dashboard_unsupported_control_ack(
	session: &DashboardWebSocketSession,
	message: &DashboardClientMessage,
	action: &str,
) -> Value {
	dashboard_control_ack_for_message(
		session,
		message,
		action,
		false,
		"unsupported_action",
		"Unsupported dashboard control action.",
	)
}

pub(super) fn dashboard_control_ack_for_message(
	session: &DashboardWebSocketSession,
	message: &DashboardClientMessage,
	action: &str,
	accepted: bool,
	status: &str,
	copy: &str,
) -> Value {
	dashboard_control_ack_value(DashboardControlAck {
		request_id: message.request_id.as_deref(),
		action,
		accepted,
		status,
		message: copy,
		project_id: message.project_id.as_deref(),
		issue_id: message.issue_id.as_deref(),
		run_id: message.run_id.as_deref(),
		subscription: Some(&session.subscription),
	})
}

pub(super) fn dashboard_control_ack_value(ack: DashboardControlAck<'_>) -> Value {
	json!({
		"requestId": ack.request_id,
		"action": ack.action,
		"accepted": ack.accepted,
		"status": ack.status,
		"message": ack.message,
		"projectId": ack.project_id,
		"issueId": ack.issue_id,
		"runId": ack.run_id,
		"subscription": ack.subscription.map(dashboard_subscription_payload),
	})
}
