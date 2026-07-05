use std::panic;

use crate::orchestrator::tests::operator::status::http::{
	self, Arc, DashboardEventHub, Mutex, OffsetDateTime, OperatorControlRequests,
	PublishedOperatorSnapshot, Read as _, Shutdown, StateStore, TcpListener, TcpStream, Write,
	orchestrator, thread,
};

#[test]
fn operator_state_endpoint_livez_ignores_poisoned_snapshot_lock() {
	let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
	let address = listener.local_addr().expect("listener address should resolve");
	let snapshot = Arc::new(Mutex::new(PublishedOperatorSnapshot {
		snapshot_json: Some(br#"{"status":"ok"}"#.to_vec()),
		last_publish_unix_epoch: Some(OffsetDateTime::now_utc().unix_timestamp()),
	}));
	let state_store = Arc::new(StateStore::open_in_memory().expect("state store should open"));
	let poisoned_snapshot = Arc::clone(&snapshot);
	let _ = panic::catch_unwind(move || {
		let _guard = poisoned_snapshot.lock().expect("snapshot lock should acquire");

		http::panic!("poison snapshot lock");
	});
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
		.expect("live probe should not require snapshot lock");
	});
	let mut client = TcpStream::connect(address).expect("client should connect");
	let mut response = String::new();

	client
		.write_all(
			format!(
				"GET {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
				orchestrator::OPERATOR_LIVE_ENDPOINT_PATH
			)
			.as_bytes(),
		)
		.expect("client should write request");
	client.shutdown(Shutdown::Write).expect("client should close the request body stream");
	client.read_to_string(&mut response).expect("client should read response");
	server.join().expect("server thread should complete");

	assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
	assert!(response.ends_with("ok"));
}

#[test]
fn operator_state_endpoint_serves_only_liveness_probe() {
	let live_response = String::from_utf8(
		orchestrator::build_operator_state_http_response(
			format!(
				"GET {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
				orchestrator::OPERATOR_LIVE_ENDPOINT_PATH
			)
			.as_bytes(),
		)
		.expect("live response should build"),
	)
	.expect("live response should be utf-8");

	assert!(live_response.starts_with("HTTP/1.1 200 OK\r\n"));
	assert!(live_response.ends_with("ok"));
}
