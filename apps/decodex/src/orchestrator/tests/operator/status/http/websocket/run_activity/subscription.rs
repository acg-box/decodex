use crate::orchestrator::tests::operator::status::http::{
	self, Arc, DashboardEventHub, Mutex, OperatorControlRequests, PublishedOperatorSnapshot,
	StateStore, TcpListener, Write as _, orchestrator, thread, websocket,
};

#[test]
fn operator_dashboard_websocket_filters_run_activity_by_subscription() {
	let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
	let address = listener.local_addr().expect("listener address should resolve");
	let snapshot = Arc::new(Mutex::new(PublishedOperatorSnapshot::default()));
	let state_store = Arc::new(StateStore::open_in_memory().expect("state store should open"));
	let dashboard_events = DashboardEventHub::default();
	let server_snapshot = Arc::clone(&snapshot);
	let server_state_store = Arc::clone(&state_store);
	let server_dashboard_events = dashboard_events.clone();
	let server = thread::spawn(move || {
		let (stream, _) = listener.accept().expect("listener should accept a connection");

		orchestrator::handle_operator_state_endpoint_connection(
			stream,
			&server_snapshot,
			&server_dashboard_events,
			&OperatorControlRequests::default(),
			&server_state_store,
		)
		.expect("websocket handler should complete after client disconnect");
	});
	let (mut client, response, mut frame) = http::open_dashboard_websocket_client(address);

	assert!(response.starts_with("HTTP/1.1 101 Switching Protocols\r\n"));

	client
		.write_all(&websocket::websocket_client_text_frame(
			r#"{"type":"subscribe","requestId":"sub-filter","projectId":"pubfi","runId":"run-2"}"#,
		))
		.expect("client should send subscription");

	let _subscribe_ack = http::read_websocket_json_until(&mut client, &mut frame, |payload| {
		payload["type"] == "controlAck" && payload["payload"]["requestId"] == "sub-filter"
	});

	dashboard_events.broadcast(
		"runActivity",
		serde_json::json!({
			"emittedAtUnixEpoch": 1_774_000_010_i64,
			"currentLanes": [
				{ "project_id": "pubfi", "issue_id": "PUB-101", "run_id": "run-1" },
				{ "project_id": "pubfi", "issue_id": "PUB-102", "run_id": "run-2" },
				{ "project_id": "rsnap", "issue_id": "RS-1", "run_id": "run-2" }
			],
			"presentation": {
				"schema": "decodex.operator.presentation/1",
				"current_lane_cards": [
					{
						"id": "run-1",
						"run_id": "run-1",
						"issue_id": "PUB-101",
						"project_id": "pubfi",
						"run": { "project_id": "pubfi", "issue_id": "PUB-101", "run_id": "run-1" }
					},
					{
						"id": "run-2",
						"run_id": "run-2",
						"issue_id": "PUB-102",
						"project_id": "pubfi",
						"run": { "project_id": "pubfi", "issue_id": "PUB-102", "run_id": "run-2" }
					},
					{
						"id": "run-2-rsnap",
						"run_id": "run-2",
						"issue_id": "RS-1",
						"project_id": "rsnap",
						"run": { "project_id": "rsnap", "issue_id": "RS-1", "run_id": "run-2" }
					}
				]
			}
		}),
	);

	let activity = http::read_websocket_json_until(&mut client, &mut frame, |payload| {
		payload["type"] == "runActivity"
	});
	let current_lanes =
		activity["payload"]["currentLanes"].as_array().expect("current lanes should list");

	assert_eq!(current_lanes.len(), 1);
	assert_eq!(current_lanes[0]["project_id"], "pubfi");
	assert_eq!(current_lanes[0]["issue_id"], "PUB-102");
	assert_eq!(current_lanes[0]["run_id"], "run-2");
	assert_eq!(activity["payload"]["currentLanesComplete"], true);
	assert_eq!(activity["payload"]["currentLaneScope"], "filtered");

	let current_lane_cards = activity["payload"]["presentation"]["current_lane_cards"]
		.as_array()
		.expect("filtered current lane cards should list");

	assert_eq!(current_lane_cards.len(), 1);
	assert_eq!(current_lane_cards[0]["issue_id"], "PUB-102");
	assert_eq!(current_lane_cards[0]["run_id"], "run-2");

	drop(client);

	dashboard_events.close_clients_for_test();
	server.join().expect("server thread should complete");
}
