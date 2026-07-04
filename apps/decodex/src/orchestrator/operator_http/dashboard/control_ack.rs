use serde_json::Value;

use crate::orchestrator::operator_http::{
	DashboardClientMessage, DashboardControlAck, DashboardWebSocketSession,
	dashboard::subscription::dashboard_subscription_payload, types::DashboardClientSubscription,
};

pub(crate) fn dashboard_control_ack_should_push_snapshot(ack: &Value) -> bool {
	ack.get("accepted").and_then(Value::as_bool).unwrap_or(false)
		&& matches!(
			ack.get("action").and_then(Value::as_str),
			Some("selectAccount" | "clearAccountSelection")
		)
}

pub(crate) fn dashboard_control_ack_should_push_run_activity(ack: &Value) -> bool {
	ack.get("accepted").and_then(Value::as_bool).unwrap_or(false)
		&& matches!(
			ack.get("action").and_then(Value::as_str),
			Some("subscribe" | "focus" | "clearFocus" | "selectAccount" | "clearAccountSelection")
		)
}

pub(crate) fn dashboard_control_ready_payload(subscription: &DashboardClientSubscription) -> Value {
	crate::orchestrator::operator_http::json!({
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

pub(crate) fn dashboard_control_ack_for_message(
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

pub(crate) fn dashboard_control_ack_value(ack: DashboardControlAck<'_>) -> Value {
	crate::orchestrator::operator_http::json!({
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
