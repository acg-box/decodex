use crate::orchestrator::tests::operator::status::http::{
	DASHBOARD_MAX_WEBSOCKET_CLIENTS, DashboardClientSubscription, DashboardEventHub, orchestrator,
};

#[test]
fn dashboard_event_hub_unregisters_websocket_clients_and_caps_fanout() {
	let hub = DashboardEventHub::default();
	let mut registrations = Vec::new();

	for _ in 0..DASHBOARD_MAX_WEBSOCKET_CLIENTS {
		registrations.push(hub.subscribe().expect("client should subscribe below cap"));
	}

	assert_eq!(hub.client_count_for_test(), orchestrator::DASHBOARD_MAX_WEBSOCKET_CLIENTS);
	assert!(
		hub.subscribe().is_err(),
		"client fanout should be capped instead of growing unbounded"
	);

	drop(registrations.pop());

	assert_eq!(hub.client_count_for_test(), orchestrator::DASHBOARD_MAX_WEBSOCKET_CLIENTS - 1);

	let replacement = hub.subscribe().expect("slot should reopen after client drop");

	assert_eq!(hub.client_count_for_test(), orchestrator::DASHBOARD_MAX_WEBSOCKET_CLIENTS);

	drop(replacement);
	drop(registrations);

	assert_eq!(hub.client_count_for_test(), 0);
}

#[test]
fn dashboard_event_hub_caches_and_filters_last_run_activity_event() {
	let hub = DashboardEventHub::default();
	let payload = serde_json::json!({
	"emittedAtUnixEpoch": 1_774_000_000,
	"accountControl": {
		"mode": "balanced",
		"account_selector": null,
	},
	"accounts": [],
	"currentLanes": [
		{
			"project_id": "decodex",
			"issue_id": "issue-1",
			"run_id": "run-1",
		},
		{
			"project_id": "decodex",
			"issue_id": "issue-2",
			"run_id": "run-2",
		},
		],
		"currentLanesComplete": true,
		"currentLaneScope": "complete",
		"presentation": {
			"schema": "decodex.operator.presentation/1",
			"current_lane_cards": [
				{
					"id": "run-1",
					"run_id": "run-1",
					"issue_id": "issue-1",
					"project_id": "decodex",
					"run": {
						"project_id": "decodex",
						"issue_id": "issue-1",
						"run_id": "run-1"
					}
				},
				{
					"id": "run-2",
					"run_id": "run-2",
					"issue_id": "issue-2",
					"project_id": "decodex",
					"run": {
						"project_id": "decodex",
						"issue_id": "issue-2",
						"run_id": "run-2"
					}
				}
			]
		},
	});
	let subscription = DashboardClientSubscription {
		project_id: Some(String::from("decodex")),
		issue_id: Some(String::from("issue-1")),
		run_id: None,
	};

	hub.broadcast("runActivity", payload);
	hub.broadcast("snapshot", serde_json::json!({"ignored": true}));

	let event = hub
		.cached_run_activity_event(&subscription)
		.expect("cached run activity should remain available after other event types");
	let current_lanes = event.payload["currentLanes"]
		.as_array()
		.expect("filtered current lanes should be an array");

	assert_eq!(event.event_type, "runActivity");
	assert_eq!(current_lanes.len(), 1);
	assert_eq!(current_lanes[0]["issue_id"], "issue-1");
	assert_eq!(event.payload["currentLanesComplete"], true);
	assert_eq!(event.payload["currentLaneScope"], "filtered");

	let current_lane_cards = event.payload["presentation"]["current_lane_cards"]
		.as_array()
		.expect("filtered current lane cards should be an array");

	assert_eq!(current_lane_cards.len(), 1);
	assert_eq!(current_lane_cards[0]["issue_id"], "issue-1");
	assert_eq!(current_lane_cards[0]["run"]["run_id"], "run-1");
}

#[test]
fn dashboard_event_hub_filtered_empty_complete_event_clears_subscribed_overlay() {
	let hub = DashboardEventHub::default();
	let payload = serde_json::json!({
	"emittedAtUnixEpoch": 1_774_000_000,
	"accountControl": {
		"mode": "balanced",
		"account_selector": null,
	},
	"accounts": [],
		"currentLanes": [
			{
				"project_id": "decodex",
				"issue_id": "issue-2",
				"run_id": "run-2",
			},
		],
		"currentLanesComplete": true,
		"currentLaneScope": "complete",
		"presentation": {
			"schema": "decodex.operator.presentation/1",
			"current_lane_cards": [
				{
					"id": "run-2",
					"run_id": "run-2",
					"issue_id": "issue-2",
					"project_id": "decodex",
					"run": {
						"project_id": "decodex",
						"issue_id": "issue-2",
						"run_id": "run-2"
					}
				}
			]
		},
	});
	let subscription = DashboardClientSubscription {
		project_id: Some(String::from("decodex")),
		issue_id: Some(String::from("issue-1")),
		run_id: None,
	};

	hub.broadcast("runActivity", payload);

	let event = hub
		.cached_run_activity_event(&subscription)
		.expect("cached run activity should remain available for empty filtered scope");
	let current_lanes = event.payload["currentLanes"]
		.as_array()
		.expect("filtered current lanes should be an array");

	assert!(current_lanes.is_empty());
	assert_eq!(event.payload["currentLanesComplete"], true);
	assert_eq!(event.payload["currentLaneScope"], "filtered");
	assert!(
		event.payload["presentation"]["current_lane_cards"]
			.as_array()
			.expect("filtered current lane cards should be an array")
			.is_empty()
	);
}
