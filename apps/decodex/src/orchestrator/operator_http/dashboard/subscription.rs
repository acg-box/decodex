use serde_json::{Value, json};

use super::super::{
	DashboardBroadcastEvent, DashboardClientMessage, types::DashboardClientSubscription,
};

pub(super) fn dashboard_subscription_from_message(
	message: &DashboardClientMessage,
) -> DashboardClientSubscription {
	DashboardClientSubscription {
		project_id: dashboard_clean_scope_value(message.project_id.as_deref()),
		issue_id: dashboard_clean_scope_value(message.issue_id.as_deref()),
		run_id: dashboard_clean_scope_value(message.run_id.as_deref()),
	}
}

pub(super) fn dashboard_subscription_payload(subscription: &DashboardClientSubscription) -> Value {
	json!({
		"projectId": subscription.project_id,
		"issueId": subscription.issue_id,
		"runId": subscription.run_id,
	})
}

pub(super) fn dashboard_required_account_selector(
	message: &DashboardClientMessage,
) -> Option<&str> {
	message.account_selector.as_deref().map(str::trim).filter(|value| !value.is_empty())
}

pub(super) fn dashboard_clean_scope_value(value: Option<&str>) -> Option<String> {
	value.map(str::trim).filter(|value| !value.is_empty()).map(str::to_owned)
}

pub(in crate::orchestrator::operator_http) fn dashboard_event_for_subscription(
	event: &DashboardBroadcastEvent,
	subscription: &DashboardClientSubscription,
) -> Option<DashboardBroadcastEvent> {
	if event.event_type != "runActivity" || dashboard_subscription_is_empty(subscription) {
		return Some(event.clone());
	}

	let current_lanes =
		event.payload.get("currentLanes").and_then(Value::as_array).map(|runs| {
			runs.iter()
				.filter(|run| dashboard_run_matches_subscription(run, subscription))
				.cloned()
				.collect::<Vec<_>>()
		})?;
	let current_lanes_complete =
		event.payload.get("currentLanesComplete").and_then(Value::as_bool).unwrap_or(true);
	let current_lane_cards = event
		.payload
		.get("presentation")
		.and_then(|presentation| presentation.get("current_lane_cards"))
		.and_then(Value::as_array)
		.map(|cards| {
			cards
				.iter()
				.filter(|card| {
					let run = card.get("run").unwrap_or(card);

					dashboard_run_matches_subscription(run, subscription)
				})
				.cloned()
				.collect::<Vec<_>>()
		});
	let mut payload = event.payload.clone();

	payload["currentLanes"] = Value::Array(current_lanes);
	payload["currentLanesComplete"] = Value::Bool(current_lanes_complete);
	payload["currentLaneScope"] = Value::String(String::from("filtered"));

	if let Some(current_lane_cards) = current_lane_cards
		&& let Some(presentation) = payload.get_mut("presentation").and_then(Value::as_object_mut)
	{
		presentation.insert(String::from("current_lane_cards"), Value::Array(current_lane_cards));
	}

	Some(DashboardBroadcastEvent { event_type: event.event_type, payload })
}

pub(super) fn dashboard_subscription_is_empty(subscription: &DashboardClientSubscription) -> bool {
	subscription.project_id.is_none()
		&& subscription.issue_id.is_none()
		&& subscription.run_id.is_none()
}

pub(super) fn dashboard_run_matches_subscription(
	run: &Value,
	subscription: &DashboardClientSubscription,
) -> bool {
	if let Some(project_id) = subscription.project_id.as_deref()
		&& run.get("project_id").and_then(Value::as_str) != Some(project_id)
	{
		return false;
	}
	if let Some(issue_id) = subscription.issue_id.as_deref()
		&& run.get("issue_id").and_then(Value::as_str) != Some(issue_id)
	{
		return false;
	}
	if let Some(run_id) = subscription.run_id.as_deref()
		&& run.get("run_id").and_then(Value::as_str) != Some(run_id)
	{
		return false;
	}

	true
}
