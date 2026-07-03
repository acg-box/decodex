use std::{
	io::Write as _,
	net::TcpStream,
	sync::{Arc, Mutex, mpsc::RecvTimeoutError},
	time::{Duration, Instant},
};

use crate::orchestrator::{
	OPERATOR_DASHBOARD_WS_HEARTBEAT_INTERVAL,
	operator_http::{
		DashboardClientFrame, DashboardEventHub, DashboardWebSocketSession,
		PublishedOperatorSnapshot, Result, StateStore,
		dashboard::{control, framing, handshake, run_activity, snapshot, subscription},
	},
};

pub(crate) fn handle_operator_dashboard_websocket_connection(
	mut stream: TcpStream,
	request: &[u8],
	snapshot: &Arc<Mutex<PublishedOperatorSnapshot>>,
	dashboard_events: &DashboardEventHub,
	state_store: &Arc<StateStore>,
) -> Result<()> {
	stream.set_read_timeout(Some(Duration::from_millis(20)))?;
	stream.set_write_timeout(Some(Duration::from_secs(2)))?;

	let response = match handshake::operator_dashboard_websocket_response_headers(request) {
		Ok(response) => response,
		Err(response) => {
			stream.write_all(&response)?;

			return Ok(());
		},
	};
	let events = dashboard_events.subscribe()?;
	let mut session = DashboardWebSocketSession::default();
	let mut client_frame_buffer = Vec::new();
	let mut last_heartbeat = Instant::now();

	stream.write_all(&response)?;

	framing::write_dashboard_websocket_event(
		&mut stream,
		"controlReady",
		&control::dashboard_control_ready_payload(&session.subscription),
	)?;

	if let Some(payload) = snapshot::dashboard_current_snapshot_event_payload(snapshot)? {
		framing::write_dashboard_websocket_event(&mut stream, "snapshot", &payload)?;
	}

	run_activity::write_cached_dashboard_run_activity_event(
		&mut stream,
		dashboard_events,
		&session.subscription,
	);

	loop {
		for frame in
			framing::read_dashboard_websocket_client_frames(&mut stream, &mut client_frame_buffer)?
		{
			match frame {
				DashboardClientFrame::Text(payload) => {
					let response = control::handle_dashboard_client_message(
						&mut session,
						state_store,
						&payload,
					);

					framing::write_dashboard_websocket_event(&mut stream, "controlAck", &response)?;

					if control::dashboard_control_ack_should_push_snapshot(&response)
						&& let Some(payload) =
							snapshot::dashboard_current_snapshot_event_payload(snapshot)?
					{
						framing::write_dashboard_websocket_event(
							&mut stream,
							"snapshot",
							&payload,
						)?;
					}
					if control::dashboard_control_ack_should_push_run_activity(&response) {
						run_activity::write_cached_dashboard_run_activity_event(
							&mut stream,
							dashboard_events,
							&session.subscription,
						);
					}
				},
				DashboardClientFrame::Close => return Ok(()),
				DashboardClientFrame::Ping(payload) => {
					stream.write_all(&framing::websocket_frame(0xA, &payload)?)?;
				},
				DashboardClientFrame::Pong => {},
			}
		}

		match events.recv_timeout(Duration::from_millis(100)) {
			Ok(event) => {
				if let Some(event) =
					subscription::dashboard_event_for_subscription(&event, &session.subscription)
				{
					framing::write_dashboard_websocket_event(
						&mut stream,
						event.event_type,
						&event.payload,
					)?;
				}
			},
			Err(RecvTimeoutError::Timeout) => {},
			Err(RecvTimeoutError::Disconnected) => return Ok(()),
		}

		if last_heartbeat.elapsed() >= OPERATOR_DASHBOARD_WS_HEARTBEAT_INTERVAL {
			stream.write_all(&framing::websocket_ping_frame())?;

			last_heartbeat = Instant::now();
		}
	}
}
