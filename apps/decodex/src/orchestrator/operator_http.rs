use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use sha1::{Digest as _, Sha1};
use libc::SIGTERM;

#[cfg(test)]
type DashboardRunInterrupterForTest = fn(u32) -> Result<()>;

const OPERATOR_DASHBOARD_HTML: &str =
	include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/orchestrator/operator_dashboard.html"));
const OPERATOR_DASHBOARD_ICON_PNG: &[u8] =
	include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../site/public/assets/icon.png"));
const OPERATOR_DASHBOARD_LOGO_ICO: &[u8] =
	include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../site/public/assets/logo.ico"));
const OPERATOR_DASHBOARD_LOGO_TOUCH_PNG: &[u8] = include_bytes!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../site/public/assets/logo-touch.png"
));

#[cfg(test)]
static DASHBOARD_RUN_INTERRUPTER_FOR_TEST: Mutex<Option<DashboardRunInterrupterForTest>> =
	Mutex::new(None);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OperatorSnapshotReadiness {
	Ready,
	SnapshotUnavailable,
	SnapshotStale,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OperatorRequestRoute {
	Dashboard,
	DashboardIconPng,
	DashboardLogoIco,
	DashboardLogoTouchPng,
	DashboardWs,
	Live,
	Ready,
	State,
}

enum DashboardClientFrame {
	Text(Vec<u8>),
	Close,
	Ping(Vec<u8>),
	Pong,
}

#[derive(Clone, Default)]
struct DashboardEventHub {
	clients: Arc<Mutex<Vec<Sender<DashboardBroadcastEvent>>>>,
}
impl DashboardEventHub {
	fn subscribe(&self) -> Result<Receiver<DashboardBroadcastEvent>> {
		let (event_tx, event_rx) = mpsc::channel();
		let mut clients = self
			.clients
			.lock()
			.map_err(|error| eyre::eyre!("Dashboard event client lock poisoned: {error}"))?;

		clients.push(event_tx);

		Ok(event_rx)
	}

	fn broadcast(&self, event_type: &'static str, payload: Value) {
		let Ok(mut clients) = self.clients.lock() else {
			tracing::warn!("Skipped dashboard event broadcast because the client list lock is poisoned.");

			return;
		};
		let event = DashboardBroadcastEvent { event_type, payload };

		clients.retain(|client| client.send(event.clone()).is_ok());
	}

	fn has_clients(&self) -> bool {
		self.clients.lock().is_ok_and(|clients| !clients.is_empty())
	}

	#[cfg(test)]
	fn close_clients_for_test(&self) {
		if let Ok(mut clients) = self.clients.lock() {
			clients.clear();
		}
	}
}

#[derive(Clone, Debug)]
struct DashboardBroadcastEvent {
	event_type: &'static str,
	payload: Value,
}

#[derive(Clone, Debug, Default)]
struct DashboardClientSubscription {
	project_id: Option<String>,
	issue_id: Option<String>,
	run_id: Option<String>,
}

#[derive(Default)]
struct DashboardWebSocketSession {
	subscription: DashboardClientSubscription,
}

#[derive(Debug, Deserialize)]
struct DashboardClientMessage {
	#[serde(rename = "type")]
	message_type: String,
	#[serde(rename = "requestId")]
	request_id: Option<String>,

	action: Option<String>,
	#[serde(rename = "projectId")]
	project_id: Option<String>,
	#[serde(rename = "issueId")]
	issue_id: Option<String>,
	#[serde(rename = "runId")]
	run_id: Option<String>,
	#[serde(rename = "accountSelector")]
	account_selector: Option<String>,
}

struct DashboardControlAck<'a> {
	request_id: Option<&'a str>,
	action: &'a str,
	accepted: bool,
	status: &'a str,
	message: &'a str,
	project_id: Option<&'a str>,
	issue_id: Option<&'a str>,
	run_id: Option<&'a str>,
	subscription: Option<&'a DashboardClientSubscription>,
}

struct DashboardRunActivityEvent {
	fingerprint: Vec<u8>,
	event: DashboardBroadcastEvent,
}

#[cfg(test)]
struct DashboardRunInterrupterGuardForTest {
	previous: Option<DashboardRunInterrupterForTest>,
}
#[cfg(test)]
impl Drop for DashboardRunInterrupterGuardForTest {
	fn drop(&mut self) {
		let mut slot = DASHBOARD_RUN_INTERRUPTER_FOR_TEST
			.lock()
			.expect("dashboard run interrupter test hook should not be poisoned");

		*slot = self.previous.take();
	}
}

fn run_operator_state_endpoint(
	listener: TcpListener,
	snapshot: Arc<Mutex<PublishedOperatorSnapshot>>,
	dashboard_events: DashboardEventHub,
	state_store: Arc<StateStore>,
	ready_stale_after: Duration,
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
				let connection_state_store = Arc::clone(&state_store);

				thread::spawn(move || {
					if let Err(error) = handle_operator_state_endpoint_connection(
						stream,
						&connection_snapshot,
						&connection_dashboard_events,
						&connection_state_store,
						ready_stale_after,
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

fn handle_operator_state_endpoint_connection(
	mut stream: TcpStream,
	snapshot: &Arc<Mutex<PublishedOperatorSnapshot>>,
	dashboard_events: &DashboardEventHub,
	state_store: &Arc<StateStore>,
	ready_stale_after: Duration,
) -> Result<()> {
	stream.set_read_timeout(Some(Duration::from_millis(250)))?;
	stream.set_write_timeout(Some(Duration::from_millis(250)))?;

	let request = read_operator_state_request_headers(&mut stream)?;
	let route = match parse_operator_state_request_route(&request) {
		Ok(route) => route,
		Err(response) => {
			stream.write_all(&response)?;

			return Ok(());
		},
	};

	if route == OperatorRequestRoute::DashboardWs {
		handle_operator_dashboard_websocket_connection(
			stream,
			&request,
			dashboard_events,
			state_store,
		)?;

		return Ok(());
	}

	let response = match route {
		OperatorRequestRoute::Dashboard
		| OperatorRequestRoute::DashboardIconPng
		| OperatorRequestRoute::DashboardLogoIco
		| OperatorRequestRoute::DashboardLogoTouchPng
		| OperatorRequestRoute::Live => {
			build_operator_state_http_response_for_route(
				route,
				None,
				None,
				OperatorSnapshotReadiness::Ready,
			)
		},
		OperatorRequestRoute::Ready => {
			let last_publish_unix_epoch = snapshot
				.lock()
				.map_err(|error| eyre::eyre!("Operator state snapshot lock poisoned: {error}"))?
				.last_publish_unix_epoch;

			build_operator_state_http_response_for_route(
				route,
				None,
				None,
				operator_snapshot_readiness(
					last_publish_unix_epoch,
					OffsetDateTime::now_utc().unix_timestamp(),
					ready_stale_after,
				),
			)
		},
		OperatorRequestRoute::State => {
			let published_snapshot = snapshot
				.lock()
				.map_err(|error| eyre::eyre!("Operator state snapshot lock poisoned: {error}"))?
				.clone();

			build_operator_state_http_response_for_route(
				route,
				published_snapshot.snapshot_json.as_deref(),
				published_snapshot.last_publish_unix_epoch,
				OperatorSnapshotReadiness::SnapshotUnavailable,
			)
		},
		OperatorRequestRoute::DashboardWs => unreachable!(
			"dashboard websocket route is handled before building one-shot responses"
		),
	};

	stream.write_all(&response)?;

	Ok(())
}

fn handle_operator_dashboard_websocket_connection(
	mut stream: TcpStream,
	request: &[u8],
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

	loop {
		for frame in read_dashboard_websocket_client_frames(&mut stream, &mut client_frame_buffer)? {
			match frame {
				DashboardClientFrame::Text(payload) => {
					let response =
						handle_dashboard_client_message(&mut session, state_store, &payload);

					write_dashboard_websocket_event(&mut stream, "controlAck", &response)?;
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
				if let Some(event) =
					dashboard_event_for_subscription(&event, &session.subscription)
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

fn run_operator_run_activity_websocket_broadcasts(
	state_store: Arc<StateStore>,
	dashboard_events: DashboardEventHub,
	shutdown_rx: Receiver<()>,
) {
	let mut last_fingerprint: Option<Vec<u8>> = None;

	loop {
		match shutdown_rx.recv_timeout(OPERATOR_RUN_ACTIVITY_STREAM_INTERVAL) {
			Ok(()) | Err(RecvTimeoutError::Disconnected) => return,
			Err(RecvTimeoutError::Timeout) => {},
		}

		if !dashboard_events.has_clients() {
			last_fingerprint = None;

			continue;
		}

		match build_operator_run_activity_event(&state_store) {
			Ok(event) => {
				if last_fingerprint.as_deref() == Some(event.fingerprint.as_slice()) {
					continue;
				}

				dashboard_events.broadcast(event.event.event_type, event.event.payload);

				last_fingerprint = Some(event.fingerprint);
			},
			Err(error) => {
				tracing::warn!(?error, "Skipped dashboard run activity event.");
			},
		}
	}
}

fn build_operator_run_activity_event(state_store: &StateStore) -> Result<DashboardRunActivityEvent> {
	let now_unix_epoch = OffsetDateTime::now_utc().unix_timestamp();
	let mut active_runs = Vec::new();

	for registration in state_store.list_projects()? {
		if !registration.enabled() {
			continue;
		}

		let project = match ServiceConfig::from_path(registration.config_path()) {
			Ok(project) => project,
			Err(error) => {
				tracing::debug!(
					project_id = registration.service_id(),
					config_path = %registration.config_path().display(),
					?error,
					"Skipped dashboard run activity for an unreadable registered project."
				);

				continue;
			},
		};
		let (runs, _) = state_store.list_project_runs(project.service_id(), 0)?;

		for run in runs {
			let run_status = operator_run_status(&project, run, now_unix_epoch)?;

			if operator_run_counts_as_active(&run_status) {
				active_runs.push(run_status);
			}
		}
	}

	let fingerprint = serde_json::to_vec(&active_runs)?;
	let payload = json!({
		"emittedAtUnixEpoch": now_unix_epoch,
		"activeRuns": active_runs,
	});

	Ok(DashboardRunActivityEvent {
		fingerprint,
		event: DashboardBroadcastEvent { event_type: "runActivity", payload },
	})
}

fn write_dashboard_websocket_event(
	stream: &mut TcpStream,
	event_type: &'static str,
	payload: &Value,
) -> Result<()> {
	stream.write_all(&dashboard_websocket_message(event_type, payload)?)?;

	Ok(())
}

fn dashboard_websocket_message(event_type: &str, payload: &Value) -> Result<Vec<u8>> {
	let message = serde_json::to_vec(&json!({
		"type": event_type,
		"payload": payload,
	}))?;

	websocket_frame(0x1, &message)
}

fn websocket_frame(opcode: u8, payload: &[u8]) -> Result<Vec<u8>> {
	let mut frame = Vec::with_capacity(payload.len().saturating_add(10));

	frame.push(0x80 | opcode);

	match payload.len() {
		length @ 0..=125 => frame.push(length as u8),
		length @ 126..=65_535 => {
			frame.push(126);
			frame.extend_from_slice(&(length as u16).to_be_bytes());
		},
		length => {
			frame.push(127);

			let length = u64::try_from(length)
				.map_err(|error| eyre::eyre!("WebSocket frame payload length overflow: {error}"))?;

			frame.extend_from_slice(&length.to_be_bytes());
		},
	}

	frame.extend_from_slice(payload);

	Ok(frame)
}

fn websocket_ping_frame() -> Vec<u8> {
	vec![0x89, 0]
}

fn read_dashboard_websocket_client_frames(
	stream: &mut TcpStream,
	buffer: &mut Vec<u8>,
) -> Result<Vec<DashboardClientFrame>> {
	let mut frames = Vec::new();
	let mut chunk = [0_u8; 2_048];

	loop {
		match stream.read(&mut chunk) {
			Ok(0) => {
				frames.push(DashboardClientFrame::Close);

				break;
			},
			Ok(bytes_read) => {
				buffer.extend_from_slice(&chunk[..bytes_read]);

				while let Some(frame) = parse_dashboard_websocket_client_frame(buffer)? {
					frames.push(frame);
				}
			},
			Err(error)
				if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
			{
				break;
			},
			Err(error) if error.kind() == ErrorKind::Interrupted => continue,
			Err(error) => return Err(error.into()),
		}
	}

	Ok(frames)
}

fn parse_dashboard_websocket_client_frame(
	buffer: &mut Vec<u8>,
) -> Result<Option<DashboardClientFrame>> {
	if buffer.len() < 2 {
		return Ok(None);
	}

	let fin = buffer[0] & 0x80 != 0;
	let opcode = buffer[0] & 0x0f;
	let masked = buffer[1] & 0x80 != 0;
	let payload_length_marker = buffer[1] & 0x7f;
	let mut offset = 2_usize;
	let payload_length = match payload_length_marker {
		length @ 0..=125 => usize::from(length),
		126 => {
			if buffer.len() < offset + 2 {
				return Ok(None);
			}

			let length = usize::from(u16::from_be_bytes([buffer[offset], buffer[offset + 1]]));

			offset += 2;

			length
		},
		127 => {
			if buffer.len() < offset + 8 {
				return Ok(None);
			}

			let length = u64::from_be_bytes([
				buffer[offset],
				buffer[offset + 1],
				buffer[offset + 2],
				buffer[offset + 3],
				buffer[offset + 4],
				buffer[offset + 5],
				buffer[offset + 6],
				buffer[offset + 7],
			]);

			offset += 8;

			usize::try_from(length)
				.map_err(|error| eyre::eyre!("WebSocket client frame length overflow: {error}"))?
		},
		_ => unreachable!("websocket payload length marker is masked to seven bits"),
	};

	if payload_length > OPERATOR_DASHBOARD_WS_CLIENT_MESSAGE_MAX_BYTES {
		eyre::bail!("WebSocket client frame exceeded the dashboard message limit.");
	}
	if !fin {
		eyre::bail!("Fragmented dashboard WebSocket messages are not supported.");
	}
	if !masked {
		eyre::bail!("Dashboard WebSocket client frame was not masked.");
	}
	if buffer.len() < offset + 4 {
		return Ok(None);
	}

	let mask = [buffer[offset], buffer[offset + 1], buffer[offset + 2], buffer[offset + 3]];

	offset += 4;

	let frame_end = offset
		.checked_add(payload_length)
		.ok_or_else(|| eyre::eyre!("WebSocket client frame length overflow."))?;

	if buffer.len() < frame_end {
		return Ok(None);
	}

	let mut payload = buffer[offset..frame_end].to_vec();

	for (index, byte) in payload.iter_mut().enumerate() {
		*byte ^= mask[index % mask.len()];
	}

	buffer.drain(..frame_end);

	let frame = match opcode {
		0x1 => DashboardClientFrame::Text(payload),
		0x8 => DashboardClientFrame::Close,
		0x9 => DashboardClientFrame::Ping(payload),
		0xA => DashboardClientFrame::Pong,
		_ => return Ok(None),
	};

	Ok(Some(frame))
}

fn dashboard_control_ready_payload(subscription: &DashboardClientSubscription) -> Value {
	json!({
		"supportedActions": [
			"subscribe",
			"focus",
			"clearFocus",
			"pauseProject",
			"resumeProject",
			"interruptRun",
			"selectAccount",
			"clearAccountSelection",
			"ack"
		],
		"subscription": dashboard_subscription_payload(subscription),
	})
}

fn handle_dashboard_client_message(
	session: &mut DashboardWebSocketSession,
	state_store: &StateStore,
	payload: &[u8],
) -> Value {
	let message = match serde_json::from_slice::<DashboardClientMessage>(payload) {
		Ok(message) => message,
		Err(error) => {
			return dashboard_control_ack_value(DashboardControlAck {
				request_id: None,
				action: "parse",
				accepted: false,
				status: "invalid_message",
				message: &format!("Dashboard control message was not valid JSON: {error}"),
				project_id: None,
				issue_id: None,
				run_id: None,
				subscription: Some(&session.subscription),
			});
		},
	};
	let action = message
		.action
		.as_deref()
		.map(str::trim)
		.filter(|value| !value.is_empty())
		.unwrap_or(message.message_type.as_str())
		.to_owned();

	match message.message_type.as_str() {
		"subscribe" => {
			session.subscription = dashboard_subscription_from_message(&message);

			dashboard_control_ack_for_message(
				session,
				&message,
				"subscribe",
				true,
				"subscribed",
				"Dashboard stream subscription updated.",
			)
		},
		"control" => handle_dashboard_control_action(session, state_store, &message, &action),
		_ => dashboard_control_ack_for_message(
			session,
			&message,
			&action,
			false,
			"unsupported_message",
			"Unsupported dashboard WebSocket message type.",
		),
	}
}

fn handle_dashboard_control_action(
	session: &mut DashboardWebSocketSession,
	state_store: &StateStore,
	message: &DashboardClientMessage,
	action: &str,
) -> Value {
	match action {
		"focus" => dashboard_focus_control_ack(session, message, action),
		"clearFocus" | "clearSubscription" =>
			dashboard_clear_focus_control_ack(session, message, action),
		"pause" | "pauseProject" =>
			dashboard_project_enabled_control_ack(session, state_store, message, action, false),
		"resume" | "resumeProject" =>
			dashboard_project_enabled_control_ack(session, state_store, message, action, true),
		"interrupt" | "interruptRun" =>
			dashboard_interrupt_control_ack(session, state_store, message, action),
		"selectAccount" =>
			dashboard_account_selection_control_ack(session, state_store, message, action, true),
		"clearAccountSelection" =>
			dashboard_account_selection_control_ack(session, state_store, message, action, false),
		"ack" | "ackNotice" => dashboard_control_ack_for_message(
			session,
			message,
			action,
			true,
			"acknowledged",
			"Dashboard acknowledgement recorded for this browser session only.",
		),
		_ => dashboard_unsupported_control_ack(session, message, action),
	}
}

fn dashboard_focus_control_ack(
	session: &mut DashboardWebSocketSession,
	message: &DashboardClientMessage,
	action: &str,
) -> Value {
	session.subscription = dashboard_subscription_from_message(message);

	dashboard_control_ack_for_message(
		session,
		message,
		action,
		true,
		"focused",
		"Dashboard focus updated for this WebSocket session.",
	)
}

fn dashboard_clear_focus_control_ack(
	session: &mut DashboardWebSocketSession,
	message: &DashboardClientMessage,
	action: &str,
) -> Value {
	session.subscription = DashboardClientSubscription::default();

	dashboard_control_ack_value(DashboardControlAck {
		request_id: message.request_id.as_deref(),
		action,
		accepted: true,
		status: "cleared",
		message: "Dashboard focus cleared for this WebSocket session.",
		project_id: None,
		issue_id: None,
		run_id: None,
		subscription: Some(&session.subscription),
	})
}

fn dashboard_project_enabled_control_ack(
	session: &DashboardWebSocketSession,
	state_store: &StateStore,
	message: &DashboardClientMessage,
	action: &str,
	enabled: bool,
) -> Value {
	let Some(project_id) = dashboard_required_project_id(message) else {
		return dashboard_missing_project_control_ack(session, message, action);
	};
	let result = state_store.set_project_enabled(project_id, enabled);
	let (accepted, status, copy) = match (enabled, result) {
		(true, Ok(())) => (true, "resumed", String::from("Project dispatch resumed.")),
		(false, Ok(())) => (
			true,
			"paused",
			String::from("Project dispatch paused; active lanes are not killed."),
		),
		(_, Err(error)) => (false, "failed", error.to_string()),
	};

	dashboard_control_ack_value(DashboardControlAck {
		request_id: message.request_id.as_deref(),
		action,
		accepted,
		status,
		message: &copy,
		project_id: Some(project_id),
		issue_id: message.issue_id.as_deref(),
		run_id: message.run_id.as_deref(),
		subscription: Some(&session.subscription),
	})
}

fn dashboard_account_selection_control_ack(
	session: &DashboardWebSocketSession,
	state_store: &StateStore,
	message: &DashboardClientMessage,
	action: &str,
	set_fixed: bool,
) -> Value {
	let Some(project_id) = dashboard_required_project_id(message) else {
		return dashboard_missing_project_control_ack(session, message, action);
	};
	let selector = if set_fixed {
		match dashboard_required_account_selector(message) {
			Some(selector) => Some(selector),
			None => {
				return dashboard_control_ack_value(DashboardControlAck {
					request_id: message.request_id.as_deref(),
					action,
					accepted: false,
					status: "missing_account",
					message: "Account selection requires an account selector.",
					project_id: Some(project_id),
					issue_id: message.issue_id.as_deref(),
					run_id: message.run_id.as_deref(),
					subscription: Some(&session.subscription),
				});
			},
		}
	} else {
		None
	};
	let result = update_project_codex_account_selection(state_store, project_id, selector);
	let (accepted, status, copy) = match (set_fixed, result) {
		(true, Ok(())) => (
			true,
			"fixed",
			String::from("Project config now pins new Codex runs to the selected account."),
		),
		(false, Ok(())) => (
			true,
			"balanced",
			String::from("Project config now uses balanced Codex account selection."),
		),
		(_, Err(error)) => (false, "failed", error.to_string()),
	};

	dashboard_control_ack_value(DashboardControlAck {
		request_id: message.request_id.as_deref(),
		action,
		accepted,
		status,
		message: &copy,
		project_id: Some(project_id),
		issue_id: message.issue_id.as_deref(),
		run_id: message.run_id.as_deref(),
		subscription: Some(&session.subscription),
	})
}

fn dashboard_interrupt_control_ack(
	session: &DashboardWebSocketSession,
	state_store: &StateStore,
	message: &DashboardClientMessage,
	action: &str,
) -> Value {
	let Some(project_id) = dashboard_required_project_id(message) else {
		return dashboard_missing_project_control_ack(session, message, action);
	};
	let Some(issue_id) = dashboard_required_issue_id(message) else {
		return dashboard_control_ack_value(DashboardControlAck {
			request_id: message.request_id.as_deref(),
			action,
			accepted: false,
			status: "missing_issue",
			message: "Stop requires an issue id.",
			project_id: Some(project_id),
			issue_id: message.issue_id.as_deref(),
			run_id: message.run_id.as_deref(),
			subscription: Some(&session.subscription),
		});
	};
	let Some(run_id) = dashboard_required_run_id(message) else {
		return dashboard_control_ack_value(DashboardControlAck {
			request_id: message.request_id.as_deref(),
			action,
			accepted: false,
			status: "missing_run",
			message: "Stop requires a run id.",
			project_id: Some(project_id),
			issue_id: Some(issue_id),
			run_id: message.run_id.as_deref(),
			subscription: Some(&session.subscription),
		});
	};

	match interrupt_dashboard_run(state_store, project_id, issue_id, run_id) {
		Ok(process_id) => dashboard_control_ack_value(DashboardControlAck {
			request_id: message.request_id.as_deref(),
			action,
			accepted: true,
			status: "interrupted",
			message: &format!("Stopped run `{run_id}` by signaling process {process_id}."),
			project_id: Some(project_id),
			issue_id: Some(issue_id),
			run_id: Some(run_id),
			subscription: Some(&session.subscription),
		}),
		Err(error) => dashboard_control_ack_value(DashboardControlAck {
			request_id: message.request_id.as_deref(),
			action,
			accepted: false,
			status: "failed",
			message: &error.to_string(),
			project_id: Some(project_id),
			issue_id: Some(issue_id),
			run_id: Some(run_id),
			subscription: Some(&session.subscription),
		}),
	}
}

fn dashboard_missing_project_control_ack(
	session: &DashboardWebSocketSession,
	message: &DashboardClientMessage,
	action: &str,
) -> Value {
	dashboard_control_ack_for_message(
		session,
		message,
		action,
		false,
		"missing_project",
		"Control action requires a project id.",
	)
}

fn dashboard_unsupported_control_ack(
	session: &DashboardWebSocketSession,
	message: &DashboardClientMessage,
	action: &str,
) -> Value {
	dashboard_control_ack_for_message(
		session,
		message,
		action,
		false,
		"unsupported_action",
		"Unsupported dashboard control action.",
	)
}

fn dashboard_control_ack_for_message(
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

fn dashboard_control_ack_value(ack: DashboardControlAck<'_>) -> Value {
	json!({
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

fn dashboard_subscription_from_message(
	message: &DashboardClientMessage,
) -> DashboardClientSubscription {
	DashboardClientSubscription {
		project_id: dashboard_clean_scope_value(message.project_id.as_deref()),
		issue_id: dashboard_clean_scope_value(message.issue_id.as_deref()),
		run_id: dashboard_clean_scope_value(message.run_id.as_deref()),
	}
}

fn dashboard_subscription_payload(subscription: &DashboardClientSubscription) -> Value {
	json!({
		"projectId": subscription.project_id,
		"issueId": subscription.issue_id,
		"runId": subscription.run_id,
	})
}

fn dashboard_required_project_id(message: &DashboardClientMessage) -> Option<&str> {
	message.project_id.as_deref().map(str::trim).filter(|value| !value.is_empty())
}

fn dashboard_required_issue_id(message: &DashboardClientMessage) -> Option<&str> {
	message.issue_id.as_deref().map(str::trim).filter(|value| !value.is_empty())
}

fn dashboard_required_run_id(message: &DashboardClientMessage) -> Option<&str> {
	message.run_id.as_deref().map(str::trim).filter(|value| !value.is_empty())
}

fn dashboard_required_account_selector(message: &DashboardClientMessage) -> Option<&str> {
	message
		.account_selector
		.as_deref()
		.map(str::trim)
		.filter(|value| !value.is_empty())
}

fn update_project_codex_account_selection(
	state_store: &StateStore,
	project_id: &str,
	selector: Option<&str>,
) -> Result<()> {
	let registration = state_store
		.list_projects()?
		.into_iter()
		.find(|project| project.service_id() == project_id)
		.ok_or_else(|| eyre::eyre!("Decodex project `{project_id}` is not registered."))?;

	write_project_codex_account_selection(registration.config_path(), selector)?;

	runtime::register_project_config(
		state_store,
		registration.config_path(),
		registration.enabled(),
	)?;

	Ok(())
}

fn write_project_codex_account_selection(
	config_path: &Path,
	selector: Option<&str>,
) -> Result<()> {
	let config_path = ServiceConfig::resolve_project_config_path(config_path)?;
	let input = fs::read_to_string(&config_path)?;
	let mut document = toml::from_str::<toml::Table>(&input)?;

	match selector.map(str::trim).filter(|value| !value.is_empty()) {
		Some(selector) => {
			let accounts = ensure_toml_table(ensure_toml_table(&mut document, "codex")?, "accounts")?;

			accounts.insert(String::from("fixed_account"), selector.to_owned().into());
		},
		None => {
			if let Some(codex) = document.get_mut("codex").and_then(|value| value.as_table_mut())
				&& let Some(accounts) = codex.get_mut("accounts").and_then(|value| value.as_table_mut())
			{
				accounts.remove("fixed_account");
			}
		},
	}

	let output = toml::to_string_pretty(&document)?;
	let temp_path = dashboard_project_config_temp_path(&config_path)?;

	fs::write(&temp_path, output)?;
	fs::rename(temp_path, &config_path)?;
	ServiceConfig::from_path(&config_path)?;

	Ok(())
}

fn ensure_toml_table<'a>(table: &'a mut toml::Table, key: &str) -> Result<&'a mut toml::Table> {
	if !table.contains_key(key) {
		table.insert(String::from(key), toml::Table::new().into());
	}

	table
		.get_mut(key)
		.and_then(|value| value.as_table_mut())
		.ok_or_else(|| eyre::eyre!("Project config `{key}` must be a TOML table."))
}

fn dashboard_project_config_temp_path(config_path: &Path) -> Result<PathBuf> {
	let parent = config_path.parent().ok_or_else(|| {
		eyre::eyre!("Project config `{}` must have a parent directory.", config_path.display())
	})?;
	let file_name = config_path
		.file_name()
		.and_then(|name| name.to_str())
		.ok_or_else(|| eyre::eyre!("Project config path must end in a valid file name."))?;

	Ok(parent.join(format!(".{file_name}.tmp-{}", process::id())))
}

fn dashboard_clean_scope_value(value: Option<&str>) -> Option<String> {
	value
		.map(str::trim)
		.filter(|value| !value.is_empty())
		.map(str::to_owned)
}

fn interrupt_dashboard_run(
	state_store: &StateStore,
	project_id: &str,
	issue_id: &str,
	run_id: &str,
) -> Result<u32> {
	let run_attempt = state_store
		.run_attempt(run_id)?
		.ok_or_else(|| eyre::eyre!("Decodex run `{run_id}` is not recorded."))?;

	if run_attempt.issue_id() != issue_id {
		eyre::bail!(
			"Decodex run `{run_id}` belongs to issue `{}`, not `{issue_id}`.",
			run_attempt.issue_id()
		);
	}
	if !matches!(run_attempt.status(), "starting" | "running") {
		eyre::bail!(
			"Decodex run `{run_id}` is `{}` and cannot be stopped.",
			run_attempt.status()
		);
	}

	let worktree = state_store
		.worktree_for_issue(issue_id)?
		.ok_or_else(|| eyre::eyre!("Issue `{issue_id}` has no recorded worktree."))?;

	if worktree.project_id() != project_id {
		eyre::bail!(
			"Issue `{issue_id}` belongs to project `{}`, not `{project_id}`.",
			worktree.project_id()
		);
	}

	let marker = state::read_run_activity_marker_snapshot(worktree.worktree_path())?
		.ok_or_else(|| eyre::eyre!("Run `{run_id}` has no activity marker."))?;

	if marker.run_id() != run_id || marker.attempt_number() != run_attempt.attempt_number() {
		eyre::bail!("Run `{run_id}` activity marker does not match the active attempt.");
	}

	let process_id = marker
		.process_id()
		.ok_or_else(|| eyre::eyre!("Run `{run_id}` has no recorded process id."))?;

	interrupt_dashboard_process(process_id)?;

	state_store.update_run_status(run_id, "interrupted")?;
	state_store.clear_lease(issue_id)?;

	Ok(process_id)
}

fn interrupt_dashboard_process(process_id: u32) -> Result<()> {
	#[cfg(test)]
	if let Some(interrupter) = *DASHBOARD_RUN_INTERRUPTER_FOR_TEST
		.lock()
		.expect("dashboard run interrupter test hook should not be poisoned")
	{
		return interrupter(process_id);
	}

	if process_id == process::id() {
		eyre::bail!("Refusing to stop the Decodex control-plane process.");
	}

	let process_id = pid_t::try_from(process_id)
		.map_err(|error| eyre::eyre!("Run process id is out of range: {error}"))?;

	if process_id <= 0 {
		eyre::bail!("Run process id must be positive.");
	}

	let result = unsafe { libc::kill(process_id, SIGTERM) };

	if result == 0 {
		return Ok(());
	}

	eyre::bail!("Failed to stop run process `{process_id}`.");
}

#[cfg(test)]
fn install_dashboard_run_interrupter_for_test(
	interrupter: DashboardRunInterrupterForTest,
) -> DashboardRunInterrupterGuardForTest {
	let mut slot = DASHBOARD_RUN_INTERRUPTER_FOR_TEST
		.lock()
		.expect("dashboard run interrupter test hook should not be poisoned");
	let previous = slot.replace(interrupter);

	DashboardRunInterrupterGuardForTest { previous }
}

fn dashboard_event_for_subscription(
	event: &DashboardBroadcastEvent,
	subscription: &DashboardClientSubscription,
) -> Option<DashboardBroadcastEvent> {
	if event.event_type != "runActivity" || dashboard_subscription_is_empty(subscription) {
		return Some(event.clone());
	}

	let active_runs = event
		.payload
		.get("activeRuns")
		.and_then(Value::as_array)
		.map(|runs| {
			runs
				.iter()
				.filter(|run| dashboard_run_matches_subscription(run, subscription))
				.cloned()
				.collect::<Vec<_>>()
		})?;
	let mut payload = event.payload.clone();

	payload["activeRuns"] = Value::Array(active_runs);

	Some(DashboardBroadcastEvent { event_type: event.event_type, payload })
}

fn dashboard_subscription_is_empty(subscription: &DashboardClientSubscription) -> bool {
	subscription.project_id.is_none() && subscription.issue_id.is_none() && subscription.run_id.is_none()
}

fn dashboard_run_matches_subscription(
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

fn operator_dashboard_websocket_response_headers(
	request: &[u8],
) -> std::result::Result<Vec<u8>, Vec<u8>> {
	let request = String::from_utf8_lossy(request);
	let Some(upgrade) = operator_http_header_value(&request, "Upgrade") else {
		return Err(websocket_upgrade_required_response());
	};
	let Some(connection) = operator_http_header_value(&request, "Connection") else {
		return Err(websocket_upgrade_required_response());
	};
	let Some(version) = operator_http_header_value(&request, "Sec-WebSocket-Version") else {
		return Err(websocket_upgrade_required_response());
	};
	let Some(key) = operator_http_header_value(&request, "Sec-WebSocket-Key") else {
		return Err(websocket_upgrade_required_response());
	};

	if !upgrade.eq_ignore_ascii_case("websocket")
		|| !operator_http_header_contains_token(connection, "upgrade")
		|| version != "13"
	{
		return Err(websocket_upgrade_required_response());
	}

	let accept_key = websocket_accept_key(key);
	let response = format!(
		"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept_key}\r\n\r\n"
	);

	Ok(response.into_bytes())
}

fn websocket_upgrade_required_response() -> Vec<u8> {
	http_response_bytes_with_headers(
		"426 Upgrade Required",
		"text/plain; charset=utf-8",
		&[("Upgrade", String::from("websocket"))],
		b"websocket upgrade required",
	)
}

fn websocket_accept_key(key: &str) -> String {
	const WEBSOCKET_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

	let mut hasher = Sha1::new();

	hasher.update(key.as_bytes());
	hasher.update(WEBSOCKET_GUID.as_bytes());

	STANDARD.encode(hasher.finalize())
}

fn operator_http_header_value<'a>(request: &'a str, header_name: &str) -> Option<&'a str> {
	request
		.lines()
		.skip(1)
		.take_while(|line| !line.trim().is_empty())
		.find_map(|line| {
			let (name, value) = line.split_once(':')?;

			name.trim()
				.eq_ignore_ascii_case(header_name)
				.then(|| value.trim())
		})
}

fn operator_http_header_contains_token(value: &str, token: &str) -> bool {
	value
		.split(',')
		.any(|candidate| candidate.trim().eq_ignore_ascii_case(token))
}

fn operator_snapshot_readiness(
	last_publish_unix_epoch: Option<i64>,
	now_unix_epoch: i64,
	ready_stale_after: Duration,
) -> OperatorSnapshotReadiness {
	let Some(last_publish_unix_epoch) = last_publish_unix_epoch else {
		return OperatorSnapshotReadiness::SnapshotUnavailable;
	};

	if last_publish_unix_epoch > now_unix_epoch {
		return OperatorSnapshotReadiness::SnapshotStale;
	}

	let Some(snapshot_age_seconds) = now_unix_epoch.checked_sub(last_publish_unix_epoch) else {
		return OperatorSnapshotReadiness::SnapshotStale;
	};
	let ready_stale_after_seconds = i64::try_from(ready_stale_after.as_secs()).unwrap_or(i64::MAX);

	if snapshot_age_seconds <= ready_stale_after_seconds {
		OperatorSnapshotReadiness::Ready
	} else {
		OperatorSnapshotReadiness::SnapshotStale
	}
}

fn read_operator_state_request_headers(stream: &mut TcpStream) -> Result<Vec<u8>> {
	let mut request = Vec::with_capacity(1_024);

	loop {
		if request
			.windows(OPERATOR_STATE_HEADER_TERMINATOR.len())
			.any(|window| window == OPERATOR_STATE_HEADER_TERMINATOR)
		{
			return Ok(request);
		}
		if request.len() >= OPERATOR_STATE_MAX_REQUEST_BYTES {
			eyre::bail!("Operator state endpoint request headers exceeded the size limit.");
		}

		let mut buffer = [0_u8; 1_024];

		match stream.read(&mut buffer) {
			Ok(0) => return Ok(request),
			Ok(bytes_read) => request.extend_from_slice(&buffer[..bytes_read]),
			Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
				eyre::bail!("Timed out while reading operator state endpoint request headers.");
			},
			Err(error) => return Err(error.into()),
		}
	}
}

#[cfg(test)]
fn build_operator_state_http_response(
	request: &[u8],
	snapshot_json: Option<&[u8]>,
	readiness: OperatorSnapshotReadiness,
) -> Result<Vec<u8>> {
	let route = match parse_operator_state_request_route(request) {
		Ok(route) => route,
		Err(response) => return Ok(response),
	};

	Ok(build_operator_state_http_response_for_route(
		route,
		snapshot_json,
		None,
		readiness,
	))
}

fn build_operator_state_http_response_for_route(
	route: OperatorRequestRoute,
	snapshot_json: Option<&[u8]>,
	snapshot_last_publish_unix_epoch: Option<i64>,
	readiness: OperatorSnapshotReadiness,
) -> Vec<u8> {
	match route {
		OperatorRequestRoute::Dashboard => {
			http_response_bytes("200 OK", "text/html; charset=utf-8", OPERATOR_DASHBOARD_HTML.as_bytes())
		},
		OperatorRequestRoute::DashboardIconPng => {
			http_response_bytes("200 OK", "image/png", OPERATOR_DASHBOARD_ICON_PNG)
		},
		OperatorRequestRoute::DashboardLogoIco => {
			http_response_bytes("200 OK", "image/x-icon", OPERATOR_DASHBOARD_LOGO_ICO)
		},
		OperatorRequestRoute::DashboardLogoTouchPng => {
			http_response_bytes("200 OK", "image/png", OPERATOR_DASHBOARD_LOGO_TOUCH_PNG)
		},
		OperatorRequestRoute::DashboardWs => websocket_upgrade_required_response(),
		OperatorRequestRoute::Live => {
			http_response_bytes("200 OK", "text/plain; charset=utf-8", b"ok")
		},
		OperatorRequestRoute::Ready => match readiness {
			OperatorSnapshotReadiness::Ready => {
				http_response_bytes("200 OK", "text/plain; charset=utf-8", b"ready")
			},
			OperatorSnapshotReadiness::SnapshotUnavailable => http_response_bytes(
				"503 Service Unavailable",
				"text/plain; charset=utf-8",
				b"snapshot_unavailable",
			),
			OperatorSnapshotReadiness::SnapshotStale => http_response_bytes(
				"503 Service Unavailable",
				"text/plain; charset=utf-8",
				b"snapshot_stale",
			),
		},
		OperatorRequestRoute::State => match snapshot_json {
			Some(snapshot_json) => {
				let headers = snapshot_response_headers(snapshot_last_publish_unix_epoch);

				http_response_bytes_with_headers("200 OK", "application/json", &headers, snapshot_json)
			},
			None => http_response_bytes(
				"503 Service Unavailable",
				"text/plain; charset=utf-8",
				b"operator snapshot unavailable",
			),
		},
	}
}

fn parse_operator_state_request_route(
	request: &[u8],
) -> std::result::Result<OperatorRequestRoute, Vec<u8>> {
	let request = String::from_utf8_lossy(request);
	let mut request_line = request.lines();
	let Some(request_line) = request_line.next() else {
		return Err(http_response_bytes(
			"400 Bad Request",
			"text/plain; charset=utf-8",
			b"missing request line",
		));
	};
	let mut parts = request_line.split_whitespace();
	let Some(method) = parts.next() else {
		return Err(http_response_bytes(
			"400 Bad Request",
			"text/plain; charset=utf-8",
			b"missing method",
		));
	};
	let Some(path) = parts.next() else {
		return Err(http_response_bytes(
			"400 Bad Request",
			"text/plain; charset=utf-8",
			b"missing path",
		));
	};

	if method != "GET" {
		return Err(http_response_bytes(
			"405 Method Not Allowed",
			"text/plain; charset=utf-8",
			b"method not allowed",
		));
	}

	let path_without_query = path
		.split_once('?')
		.map_or(path, |(path_without_query, _)| path_without_query);
	let normalized_path = path_without_query
		.split_once('#')
		.map_or(path_without_query, |(path_without_fragment, _)| path_without_fragment);

	match normalized_path {
		OPERATOR_DASHBOARD_ENDPOINT_PATH | OPERATOR_DASHBOARD_ALIAS_ENDPOINT_PATH =>
			Ok(OperatorRequestRoute::Dashboard),
		"/assets/icon.png" => Ok(OperatorRequestRoute::DashboardIconPng),
		"/assets/logo.ico" => Ok(OperatorRequestRoute::DashboardLogoIco),
		"/assets/logo-touch.png" => Ok(OperatorRequestRoute::DashboardLogoTouchPng),
		OPERATOR_DASHBOARD_WS_ENDPOINT_PATH => Ok(OperatorRequestRoute::DashboardWs),
		OPERATOR_LIVE_ENDPOINT_PATH => Ok(OperatorRequestRoute::Live),
		OPERATOR_READY_ENDPOINT_PATH => Ok(OperatorRequestRoute::Ready),
		OPERATOR_STATE_ENDPOINT_PATH => Ok(OperatorRequestRoute::State),
		_ => Err(http_response_bytes(
			"404 Not Found",
			"text/plain; charset=utf-8",
			b"not found",
		)),
	}
}

fn snapshot_response_headers(
	last_publish_unix_epoch: Option<i64>,
) -> Vec<(&'static str, String)> {
	last_publish_unix_epoch
		.map(|last_publish_unix_epoch| {
			vec![("X-Decodex-Snapshot-Unix-Epoch", last_publish_unix_epoch.to_string())]
		})
		.unwrap_or_default()
}

fn http_response_bytes(status_line: &str, content_type: &str, body: &[u8]) -> Vec<u8> {
	http_response_bytes_with_headers(status_line, content_type, &[], body)
}

fn http_response_bytes_with_headers(
	status_line: &str,
	content_type: &str,
	extra_headers: &[(&str, String)],
	body: &[u8],
) -> Vec<u8> {
	let mut response = format!(
		"HTTP/1.1 {status_line}\r\nContent-Type: {content_type}\r\n"
	)
	.into_bytes();

	for (header, value) in extra_headers {
		response.extend_from_slice(format!("{header}: {value}\r\n").as_bytes());
	}

	response.extend_from_slice(
		format!("Content-Length: {}\r\nConnection: close\r\n\r\n", body.len()).as_bytes(),
	);
	response.extend_from_slice(body);

	response
}
