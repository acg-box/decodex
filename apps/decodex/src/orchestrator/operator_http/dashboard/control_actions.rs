use serde_json::Value;

use crate::{
	accounts,
	orchestrator::operator_http::{
		DashboardClientMessage, DashboardControlAck, DashboardWebSocketSession, StateStore,
		dashboard::{control_ack, subscription},
		types::DashboardClientSubscription,
	},
};

pub(crate) fn handle_dashboard_control_action(
	session: &mut DashboardWebSocketSession,
	state_store: &StateStore,
	message: &DashboardClientMessage,
	action: &str,
) -> Value {
	match action {
		"focus" => dashboard_focus_control_ack(session, message, action),
		"clearFocus" | "clearSubscription" => {
			dashboard_clear_focus_control_ack(session, message, action)
		},
		"selectAccount" => {
			dashboard_account_selection_control_ack(session, state_store, message, action, true)
		},
		"clearAccountSelection" => {
			dashboard_account_selection_control_ack(session, state_store, message, action, false)
		},
		"ack" | "ackNotice" => control_ack::dashboard_control_ack_for_message(
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

fn dashboard_focus_control_ack(
	session: &mut DashboardWebSocketSession,
	message: &DashboardClientMessage,
	action: &str,
) -> Value {
	session.subscription = subscription::dashboard_subscription_from_message(message);

	control_ack::dashboard_control_ack_for_message(
		session,
		message,
		action,
		true,
		"focused",
		"Dashboard focus updated for this WebSocket session.",
	)
}

fn dashboard_clear_focus_control_ack(
	session: &mut DashboardWebSocketSession,
	message: &DashboardClientMessage,
	action: &str,
) -> Value {
	session.subscription = DashboardClientSubscription::default();

	control_ack::dashboard_control_ack_value(DashboardControlAck {
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

fn dashboard_account_selection_control_ack(
	session: &DashboardWebSocketSession,
	_state_store: &StateStore,
	message: &DashboardClientMessage,
	action: &str,
	set_fixed: bool,
) -> Value {
	let selector = if set_fixed {
		match subscription::dashboard_required_account_selector(message) {
			Some(selector) => Some(selector),
			None => {
				return control_ack::dashboard_control_ack_value(DashboardControlAck {
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

	control_ack::dashboard_control_ack_value(DashboardControlAck {
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

fn dashboard_unsupported_control_ack(
	session: &DashboardWebSocketSession,
	message: &DashboardClientMessage,
	action: &str,
) -> Value {
	control_ack::dashboard_control_ack_for_message(
		session,
		message,
		action,
		false,
		"unsupported_action",
		"Unsupported dashboard control action.",
	)
}
