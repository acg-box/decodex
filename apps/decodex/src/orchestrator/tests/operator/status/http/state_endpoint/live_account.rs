use crate::orchestrator::tests::operator::status::http::{
	self, Arc, DashboardEventHub, Mutex, OperatorControlRequests, PublishedOperatorSnapshot,
	StateStore, TcpListener, TempDir, TestEnvVarGuard, orchestrator, runtime, thread,
};

#[test]
fn operator_state_endpoint_overlays_live_account_control_on_published_snapshot() {
	const SNAPSHOT_UNIX_EPOCH: i64 = 1_774_000_000;

	let temp_dir = TempDir::new().expect("temp dir should exist");
	let _home_guard =
		TestEnvVarGuard::set("HOME", temp_dir.path().to_str().expect("temp path should be UTF-8"));
	let stale_snapshot = serde_json::json!({
		"project_id": "all",
		"account_control": {
			"mode": "balanced",
			"account_selector": null,
		},
		"accounts": [],
		"current_lanes": [],
		"recent_runs": [],
		"history_lanes": [],
		"queued_candidates": [],
		"worktrees": [],
		"post_review_lanes": [],
	});

	runtime::write_global_fixed_account_selector(Some("copy@example.com"))
		.expect("global account selector should write");

	let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
	let address = listener.local_addr().expect("listener address should resolve");
	let snapshot = Arc::new(Mutex::new(PublishedOperatorSnapshot {
		snapshot_json: Some(
			serde_json::to_vec(&stale_snapshot).expect("snapshot should serialize"),
		),
		last_publish_unix_epoch: Some(SNAPSHOT_UNIX_EPOCH),
	}));
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
		.expect("handler should serve websocket snapshot");
	});
	let (mut client, response, mut frame) = http::open_dashboard_websocket_client(address);
	let served_snapshot = http::read_websocket_json_until(&mut client, &mut frame, |payload| {
		payload["type"] == "snapshot"
	});

	assert!(response.starts_with("HTTP/1.1 101 Switching Protocols\r\n"));
	assert_eq!(served_snapshot["payload"]["snapshot"]["account_control"]["mode"], "fixed");
	assert_eq!(
		served_snapshot["payload"]["snapshot"]["account_control"]["account_selector"],
		"copy@example.com"
	);
	assert_eq!(served_snapshot["payload"]["snapshot"]["project_id"], "all");
	assert_eq!(served_snapshot["payload"]["snapshotPublishedAtUnixEpoch"], SNAPSHOT_UNIX_EPOCH);

	drop(client);

	dashboard_events.close_clients_for_test();
	server.join().expect("server thread should complete");
}
