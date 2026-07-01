use std::{
	io::Write,
	net::TcpStream,
	sync::{Arc, Mutex, mpsc::RecvTimeoutError},
	time::{Duration, Instant},
};

use super::{
	super::{
		DashboardClientFrame, DashboardEventHub, DashboardWebSocketSession,
		PublishedOperatorSnapshot, Result, StateStore, dashboard_current_snapshot_event_payload,
	},
	control::{
		dashboard_control_ack_should_push_run_activity, dashboard_control_ack_should_push_snapshot,
		dashboard_control_ready_payload, handle_dashboard_client_message,
	},
	framing::{
		read_dashboard_websocket_client_frames, websocket_frame, websocket_ping_frame,
		write_dashboard_websocket_event,
	},
	handshake::operator_dashboard_websocket_response_headers,
	run_activity::write_cached_dashboard_run_activity_event,
	subscription::dashboard_event_for_subscription,
};
use crate::orchestrator::OPERATOR_DASHBOARD_WS_HEARTBEAT_INTERVAL;

pub(in crate::orchestrator::operator_http) fn handle_operator_dashboard_websocket_connection(
	mut stream: TcpStream,
	request: &[u8],
	snapshot: &Arc<Mutex<PublishedOperatorSnapshot>>,
	dashboard_events: &DashboardEventHub,
	state_store: &Arc<StateStore>,
) -> Result<()> {
	stream.set_read_timeout(Some(Duration::from_millis(20)))?;
	stream.set_write_timeout(Some(Duration::from_secs(2)))?;

	let response = match operator_dashboard_websocket_response_headers(request) {
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

	write_dashboard_websocket_event(
		&mut stream,
		"controlReady",
		&dashboard_control_ready_payload(&session.subscription),
	)?;

	if let Some(payload) = dashboard_current_snapshot_event_payload(snapshot)? {
		write_dashboard_websocket_event(&mut stream, "snapshot", &payload)?;
	}

	write_cached_dashboard_run_activity_event(&mut stream, dashboard_events, &session.subscription);

	loop {
		for frame in read_dashboard_websocket_client_frames(&mut stream, &mut client_frame_buffer)?
		{
			match frame {
				DashboardClientFrame::Text(payload) => {
					let response =
						handle_dashboard_client_message(&mut session, state_store, &payload);

					write_dashboard_websocket_event(&mut stream, "controlAck", &response)?;

					if dashboard_control_ack_should_push_snapshot(&response)
						&& let Some(payload) = dashboard_current_snapshot_event_payload(snapshot)?
					{
						write_dashboard_websocket_event(&mut stream, "snapshot", &payload)?;
					}
					if dashboard_control_ack_should_push_run_activity(&response) {
						write_cached_dashboard_run_activity_event(
							&mut stream,
							dashboard_events,
							&session.subscription,
						);
					}
				},
				DashboardClientFrame::Close => return Ok(()),
				DashboardClientFrame::Ping(payload) => {
					stream.write_all(&websocket_frame(0xA, &payload)?)?;
				},
				DashboardClientFrame::Pong => {},
			}
		}

		match events.recv_timeout(Duration::from_millis(100)) {
			Ok(event) => {
				if let Some(event) = dashboard_event_for_subscription(&event, &session.subscription)
				{
					write_dashboard_websocket_event(&mut stream, event.event_type, &event.payload)?;
				}
			},
			Err(RecvTimeoutError::Timeout) => {},
			Err(RecvTimeoutError::Disconnected) => return Ok(()),
		}

		if last_heartbeat.elapsed() >= OPERATOR_DASHBOARD_WS_HEARTBEAT_INTERVAL {
			stream.write_all(&websocket_ping_frame())?;

			last_heartbeat = Instant::now();
		}
	}
}
