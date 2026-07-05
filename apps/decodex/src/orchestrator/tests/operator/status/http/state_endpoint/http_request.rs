use crate::orchestrator::tests::operator::status::http::{
	self, Arc, DashboardEventHub, Duration, ErrorKind, Mutex, OperatorControlRequests,
	PublishedOperatorSnapshot, Read as _, Shutdown, StateStore, TcpListener, TcpStream, TempDir,
	TestEnvVarGuard, Value, Write, orchestrator, thread,
};

#[test]
fn operator_state_endpoint_reads_complete_headers_before_parsing() {
	let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
	let address = listener.local_addr().expect("listener address should resolve");
	let snapshot = Arc::new(Mutex::new(PublishedOperatorSnapshot::default()));
	let state_store = Arc::new(StateStore::open_in_memory().expect("state store should open"));
	let server_snapshot = Arc::clone(&snapshot);
	let server_state_store = Arc::clone(&state_store);
	let server = thread::spawn(move || {
		let (stream, _) = listener.accept().expect("listener should accept a connection");
		let dashboard_events = DashboardEventHub::default();

		orchestrator::handle_operator_state_endpoint_connection(
			stream,
			&server_snapshot,
			&dashboard_events,
			&OperatorControlRequests::default(),
			&server_state_store,
		)
		.expect("handler should accept segmented headers");
	});
	let mut client = TcpStream::connect(address).expect("client should connect");
	let mut response = String::new();

	client.write_all(b"GET /dash").expect("client should write first request fragment");

	thread::sleep(Duration::from_millis(10));

	client
		.write_all(b"board HTTP/1.1\r\nHost: localhost\r\n\r\n")
		.expect("client should write second request fragment");
	client.shutdown(Shutdown::Write).expect("client should close the request body stream");
	client.read_to_string(&mut response).expect("client should read response");
	server.join().expect("server thread should complete");

	assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
	assert!(response.contains("<title>Decodex</title>"));
}

#[test]
fn operator_state_endpoint_serves_large_app_snapshot_without_truncation() {
	const SNAPSHOT_UNIX_EPOCH: i64 = 1_774_000_000;

	let temp_dir = TempDir::new().expect("temp dir should exist");
	let _home_guard =
		TestEnvVarGuard::set("HOME", temp_dir.path().to_str().expect("temp path should be UTF-8"));
	let large_title = "x".repeat(2_000_000);
	let snapshot_value = serde_json::json!({
		"project_id": "all",
		"account_control": {
			"mode": "balanced",
			"account_selector": null,
		},
		"accounts": [],
		"current_lanes": [],
		"recent_runs": [],
		"history_lanes": [{
			"issue_identifier": "PUB-100",
			"title": large_title,
		}],
		"queued_candidates": [],
		"worktrees": [],
		"post_review_lanes": [],
	});
	let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");

	listener.set_nonblocking(true).expect("listener should be nonblocking");

	let address = listener.local_addr().expect("listener address should resolve");
	let snapshot = Arc::new(Mutex::new(PublishedOperatorSnapshot {
		snapshot_json: Some(
			serde_json::to_vec(&snapshot_value).expect("snapshot should serialize"),
		),
		last_publish_unix_epoch: Some(SNAPSHOT_UNIX_EPOCH),
	}));
	let state_store = Arc::new(StateStore::open_in_memory().expect("state store should open"));
	let dashboard_events = DashboardEventHub::default();
	let server_snapshot = Arc::clone(&snapshot);
	let server_state_store = Arc::clone(&state_store);
	let server_dashboard_events = dashboard_events.clone();
	let server = thread::spawn(move || {
		let stream = loop {
			match listener.accept() {
				Ok((stream, _)) => break stream,
				Err(error) if error.kind() == ErrorKind::WouldBlock => {
					thread::sleep(Duration::from_millis(5));
				},
				Err(error) => http::panic!("listener should accept a connection: {error}"),
			}
		};

		orchestrator::handle_operator_state_endpoint_connection(
			stream,
			&server_snapshot,
			&server_dashboard_events,
			&OperatorControlRequests::default(),
			&server_state_store,
		)
		.expect("handler should serve the complete large app snapshot");
	});
	let mut client = TcpStream::connect(address).expect("client should connect");
	let mut response = Vec::new();

	client
		.write_all(
			format!(
				"GET {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
				orchestrator::OPERATOR_APP_SNAPSHOT_ENDPOINT_PATH
			)
			.as_bytes(),
		)
		.expect("client should write request");
	client.shutdown(Shutdown::Write).expect("client should close request body");

	thread::sleep(Duration::from_millis(350));

	client.read_to_end(&mut response).expect("client should read response");
	server.join().expect("server thread should complete");

	let header_end = response
		.windows(4)
		.position(|window| window == b"\r\n\r\n")
		.expect("response should contain header terminator");
	let headers = String::from_utf8(response[..header_end].to_vec())
		.expect("response headers should be utf-8");
	let content_length = headers
		.lines()
		.find_map(|line| line.strip_prefix("Content-Length: "))
		.expect("response should contain Content-Length")
		.parse::<usize>()
		.expect("Content-Length should parse");
	let body = &response[header_end + 4..];
	let body_json: Value = serde_json::from_slice(body).expect("body should be complete JSON");

	assert!(headers.starts_with("HTTP/1.1 200 OK\r\n"));
	assert_eq!(body.len(), content_length);
	assert_eq!(body_json["history_lanes"][0]["issue_identifier"], "PUB-100");
	assert_eq!(
		body_json["snapshotPublishedAtUnixEpoch"],
		Value::Null,
		"published epoch remains an HTTP header for app snapshot responses"
	);
}
