use super::{
	Arc, DashboardEventHub, Duration, ErrorKind, Mutex, OPERATOR_HTTP_READ_TIMEOUT,
	OperatorControlRequests, OperatorRequestRoute, PublishedOperatorSnapshot, Receiver, Result,
	StateStore, TcpListener, TcpStream, Write,
	api::{
		build_operator_lane_inspect_http_response, build_operator_lane_interrupt_http_response,
		build_operator_lane_steer_http_response,
	},
	build_operator_account_http_response, build_operator_app_snapshot_http_response,
	build_operator_linear_scan_http_response, build_operator_state_http_response_for_route,
	handle_operator_dashboard_websocket_connection, operator_request_route_is_account_api,
	parse_operator_state_request_route, read_operator_state_request_headers, thread,
};

pub(crate) fn run_operator_state_endpoint(
	listener: TcpListener,
	snapshot: Arc<Mutex<PublishedOperatorSnapshot>>,
	dashboard_events: DashboardEventHub,
	control_requests: OperatorControlRequests,
	state_store: Arc<StateStore>,
	shutdown_rx: Receiver<()>,
) {
	loop {
		if shutdown_rx.try_recv().is_ok() {
			return;
		}

		match listener.accept() {
			Ok((stream, _peer_addr)) => {
				let connection_snapshot = Arc::clone(&snapshot);
				let connection_dashboard_events = dashboard_events.clone();
				let connection_control_requests = control_requests.clone();
				let connection_state_store = Arc::clone(&state_store);

				thread::spawn(move || {
					if let Err(error) = handle_operator_state_endpoint_connection(
						stream,
						&connection_snapshot,
						&connection_dashboard_events,
						&connection_control_requests,
						&connection_state_store,
					) {
						tracing::warn!(?error, "Operator state endpoint request failed.");
					}
				});
			},
			Err(error) if error.kind() == ErrorKind::WouldBlock => {
				thread::sleep(Duration::from_millis(20));
			},
			Err(error) => {
				tracing::warn!(?error, "Operator state endpoint accept failed.");

				thread::sleep(Duration::from_millis(50));
			},
		}
	}
}

pub(crate) fn handle_operator_state_endpoint_connection(
	mut stream: TcpStream,
	snapshot: &Arc<Mutex<PublishedOperatorSnapshot>>,
	dashboard_events: &DashboardEventHub,
	control_requests: &OperatorControlRequests,
	state_store: &Arc<StateStore>,
) -> Result<()> {
	stream.set_nonblocking(false)?;
	stream.set_read_timeout(Some(OPERATOR_HTTP_READ_TIMEOUT))?;
	stream.set_write_timeout(None)?;

	let request = read_operator_state_request_headers(&mut stream)?;
	let route = match parse_operator_state_request_route(&request) {
		Ok(route) => route,
		Err(response) => {
			stream.write_all(&response)?;

			return Ok(());
		},
	};

	if operator_request_route_is_account_api(&route) {
		let response = build_operator_account_http_response(route, &request);

		stream.write_all(&response)?;

		return Ok(());
	}
	if route == OperatorRequestRoute::LinearScan {
		let response = build_operator_linear_scan_http_response(control_requests, &request);

		stream.write_all(&response)?;

		return Ok(());
	}
	if route == OperatorRequestRoute::LaneInspect {
		let response = build_operator_lane_inspect_http_response(state_store, &request);

		stream.write_all(&response)?;

		return Ok(());
	}
	if route == OperatorRequestRoute::LaneInterrupt {
		let response = build_operator_lane_interrupt_http_response(state_store, &request);

		stream.write_all(&response)?;

		return Ok(());
	}
	if route == OperatorRequestRoute::LaneSteer {
		let response = build_operator_lane_steer_http_response(state_store, &request);

		stream.write_all(&response)?;

		return Ok(());
	}
	if route == OperatorRequestRoute::AppSnapshot {
		let response = build_operator_app_snapshot_http_response(snapshot);

		stream.write_all(&response)?;

		return Ok(());
	}
	if route == OperatorRequestRoute::DashboardWs {
		handle_operator_dashboard_websocket_connection(
			stream,
			&request,
			snapshot,
			dashboard_events,
			state_store,
		)?;

		return Ok(());
	}

	let response = build_operator_state_http_response_for_route(route);

	stream.write_all(&response)?;

	Ok(())
}
