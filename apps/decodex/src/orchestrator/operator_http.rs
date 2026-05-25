use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use sha1::{Digest as _, Sha1};
use libc::SIGTERM;

use crate::accounts;
use crate::accounts::AccountUseRequest;

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
const OPERATOR_HTTP_READ_TIMEOUT: Duration = Duration::from_millis(250);

#[cfg(test)]
static DASHBOARD_RUN_INTERRUPTER_FOR_TEST: Mutex<Option<DashboardRunInterrupterForTest>> =
	Mutex::new(None);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OperatorRequestRoute {
	Dashboard,
	DashboardIconPng,
	DashboardLogoIco,
	DashboardLogoTouchPng,
	DashboardWs,
	Live,
	AppSnapshot,
	AccountList { force_refresh: bool },
	AccountSelect,
	AccountClear,
	AccountLogout,
	AccountImport,
	AccountUse,
	AccountRerollName,
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

#[derive(Deserialize)]
struct OperatorAccountRequest {
	selector: Option<String>,
	auth_json_path: Option<String>,
	random_name_offset: Option<i64>,
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

fn snapshot_json_with_live_account_control(snapshot_json: &[u8]) -> Vec<u8> {
	let Ok(mut snapshot) = serde_json::from_slice::<Value>(snapshot_json) else {
		return snapshot_json.to_vec();
	};
	let Some(snapshot_object) = snapshot.as_object_mut() else {
		return snapshot_json.to_vec();
	};

	if !snapshot_object.contains_key("account_control") {
		return snapshot_json.to_vec();
	}

	let account_control = global_codex_account_control_status();

	snapshot_object.insert(
		String::from("account_control"),
		json!({
			"mode": account_control.mode,
			"account_selector": account_control.account_selector,
		}),
	);

	match serde_json::to_vec(&snapshot) {
		Ok(output) => output,
		Err(_) => snapshot_json.to_vec(),
	}
}

fn build_operator_app_snapshot_http_response(snapshot: &Arc<Mutex<PublishedOperatorSnapshot>>) -> Vec<u8> {
	let snapshot = match snapshot.lock() {
		Ok(snapshot) => snapshot,
		Err(error) => {
			return http_response_bytes(
				"500 Internal Server Error",
				"text/plain; charset=utf-8",
				format!("operator snapshot lock poisoned: {error}").as_bytes(),
			);
		},
	};
	let Some(snapshot_json) = snapshot.snapshot_json.as_deref() else {
		return http_response_bytes_with_headers(
			"200 OK",
			"application/json",
			&[("Cache-Control", String::from("no-store"))],
			b"{}",
		);
	};
	let body = snapshot_json_with_live_account_control(snapshot_json);
	let mut headers = vec![("Cache-Control", String::from("no-store"))];

	if let Some(published_at) = snapshot.last_publish_unix_epoch {
		headers.push(("X-Decodex-Snapshot-Unix-Epoch", published_at.to_string()));
	}

	http_response_bytes_with_headers("200 OK", "application/json", &headers, &body)
}

fn handle_operator_dashboard_websocket_connection(
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

	write_current_dashboard_run_activity_event(&mut stream, state_store, &session.subscription);

	loop {
		for frame in read_dashboard_websocket_client_frames(&mut stream, &mut client_frame_buffer)? {
			match frame {
				DashboardClientFrame::Text(payload) => {
					let response = handle_dashboard_client_message(&mut session, state_store, &payload);

					write_dashboard_websocket_event(&mut stream, "controlAck", &response)?;

					if dashboard_control_ack_should_push_snapshot(&response)
						&& let Some(payload) = dashboard_current_snapshot_event_payload(snapshot)?
					{
						write_dashboard_websocket_event(&mut stream, "snapshot", &payload)?;
					}
					if dashboard_control_ack_should_push_run_activity(&response) {
						write_current_dashboard_run_activity_event(
							&mut stream,
							state_store,
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
	let account_control = global_codex_account_control_status();
	let mut accounts = Vec::new();
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
		let project_display_name = operator_project_display_name(&project);
		let mut project_active_runs = Vec::new();

		for run in runs {
			let run_status =
				operator_run_status(&project, &project_display_name, run, now_unix_epoch)?;

			if operator_run_counts_as_active(&run_status) {
				project_active_runs.push(run_status);
			}
		}

		if project_active_runs.is_empty() {
			continue;
		}

		let mut account_warnings = Vec::new();

		accounts.extend(codex_account_activity_summaries(&project, &mut account_warnings));
		active_runs.extend(project_active_runs);
	}

	let fingerprint_payload = json!({
		"accountControl": &account_control,
		"accounts": &accounts,
		"activeRuns": &active_runs,
	});
	let fingerprint = serde_json::to_vec(&fingerprint_payload)?;
	let payload = json!({
		"emittedAtUnixEpoch": now_unix_epoch,
		"accountControl": account_control,
		"accounts": accounts,
		"activeRuns": active_runs,
	});

	Ok(DashboardRunActivityEvent {
		fingerprint,
		event: DashboardBroadcastEvent { event_type: "runActivity", payload },
	})
}

fn dashboard_current_snapshot_event_payload(
	snapshot: &Arc<Mutex<PublishedOperatorSnapshot>>,
) -> Result<Option<Value>> {
	let published_snapshot = snapshot
		.lock()
		.map_err(|error| eyre::eyre!("Operator state snapshot lock poisoned: {error}"))?
		.clone();
	let Some(snapshot_json) = published_snapshot.snapshot_json.as_ref() else {
		return Ok(None);
	};
	let snapshot_json = snapshot_json_with_live_account_control(snapshot_json);
	let snapshot = serde_json::from_slice::<Value>(&snapshot_json)?;

	Ok(Some(json!({
		"snapshotPublishedAtUnixEpoch": published_snapshot.last_publish_unix_epoch,
		"snapshot": snapshot,
	})))
}

fn dashboard_control_ack_should_push_snapshot(ack: &Value) -> bool {
	ack.get("accepted").and_then(Value::as_bool).unwrap_or(false)
		&& matches!(
			ack.get("action").and_then(Value::as_str),
			Some("selectAccount" | "clearAccountSelection")
		)
}

fn dashboard_control_ack_should_push_run_activity(ack: &Value) -> bool {
	ack.get("accepted").and_then(Value::as_bool).unwrap_or(false)
		&& matches!(
			ack.get("action").and_then(Value::as_str),
			Some("subscribe" | "focus" | "clearFocus" | "selectAccount" | "clearAccountSelection")
		)
}

fn write_current_dashboard_run_activity_event(
	stream: &mut TcpStream,
	state_store: &StateStore,
	subscription: &DashboardClientSubscription,
) {
	match build_operator_run_activity_event(state_store).and_then(|event| {
		if let Some(event) = dashboard_event_for_subscription(&event.event, subscription) {
			if !dashboard_run_activity_event_has_active_runs(&event) {
				return Ok(());
			}

			write_dashboard_websocket_event(stream, event.event_type, &event.payload)?;
		}

		Ok(())
	}) {
		Ok(()) => {},
		Err(error) => {
			tracing::warn!(
				?error,
				"Skipped immediate dashboard run activity snapshot for a WebSocket client."
			);
		},
	}
}

fn dashboard_run_activity_event_has_active_runs(event: &DashboardBroadcastEvent) -> bool {
	event
		.payload
		.get("activeRuns")
		.and_then(Value::as_array)
		.is_some_and(|runs| !runs.is_empty())
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
	_state_store: &StateStore,
	message: &DashboardClientMessage,
	action: &str,
	set_fixed: bool,
) -> Value {
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
					project_id: None,
					issue_id: message.issue_id.as_deref(),
					run_id: message.run_id.as_deref(),
					subscription: Some(&session.subscription),
				});
			},
		}
	} else {
		None
	};
	let result = if let Some(selector) = selector {
		accounts::account_select(selector).map(|_| ())
	} else {
		accounts::account_clear().map(|_| ())
	};
	let (accepted, status, copy) = match (set_fixed, result) {
		(true, Ok(())) => (
			true,
			"fixed",
			String::from("Global Codex account pool now pins new runs to the selected account."),
		),
		(false, Ok(())) => (
			true,
			"balanced",
			String::from("Global Codex account pool now uses balanced account selection."),
		),
		(_, Err(error)) => (false, "failed", error.to_string()),
	};

	dashboard_control_ack_value(DashboardControlAck {
		request_id: message.request_id.as_deref(),
		action,
		accepted,
		status,
		message: &copy,
		project_id: None,
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

fn read_operator_state_request_headers(stream: &mut TcpStream) -> Result<Vec<u8>> {
	let mut request = Vec::with_capacity(1_024);

	loop {
		if let Some(header_end) = request
			.windows(OPERATOR_STATE_HEADER_TERMINATOR.len())
			.position(|window| window == OPERATOR_STATE_HEADER_TERMINATOR)
		{
			let body_offset = header_end + OPERATOR_STATE_HEADER_TERMINATOR.len();
			let content_length = operator_http_content_length(&request[..body_offset])?;

			if request.len() >= body_offset + content_length {
				return Ok(request);
			}
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

fn operator_http_content_length(headers: &[u8]) -> Result<usize> {
	let headers = String::from_utf8_lossy(headers);

	for line in headers.lines().skip(1) {
		let Some((name, value)) = line.split_once(':') else {
			continue;
		};

		if name.trim().eq_ignore_ascii_case("Content-Length") {
			return value.trim().parse::<usize>().map_err(|error| {
				eyre::eyre!("Operator HTTP request Content-Length was invalid: {error}")
			});
		}
	}

	Ok(0)
}

#[cfg(test)]
fn build_operator_state_http_response(request: &[u8]) -> Result<Vec<u8>> {
	let route = match parse_operator_state_request_route(request) {
		Ok(route) => route,
		Err(response) => return Ok(response),
	};

	if operator_request_route_is_account_api(&route) {
		return Ok(build_operator_account_http_response(route, request));
	}

	Ok(build_operator_state_http_response_for_route(route))
}

fn operator_request_route_is_account_api(route: &OperatorRequestRoute) -> bool {
	matches!(
		route,
		OperatorRequestRoute::AccountList { .. }
			| OperatorRequestRoute::AccountSelect
			| OperatorRequestRoute::AccountClear
			| OperatorRequestRoute::AccountLogout
			| OperatorRequestRoute::AccountImport
			| OperatorRequestRoute::AccountUse
			| OperatorRequestRoute::AccountRerollName
	)
}

fn build_operator_account_http_response(route: OperatorRequestRoute, request: &[u8]) -> Vec<u8> {
	match operator_account_http_response_body(route, request) {
		Ok(body) => http_response_bytes("200 OK", "application/json", &body),
		Err(error) => {
			let body = serde_json::to_vec(&json!({ "error": error.to_string() }))
				.unwrap_or_else(|_| br#"{"error":"account request failed"}"#.to_vec());

			http_response_bytes("400 Bad Request", "application/json", &body)
		},
	}
}

fn operator_account_http_response_body(
	route: OperatorRequestRoute,
	request: &[u8],
) -> Result<Vec<u8>> {
	match route {
		OperatorRequestRoute::AccountList { force_refresh } =>
			serde_json::to_vec(&accounts::account_list_with_cached_usage(force_refresh)?)
				.map_err(Into::into),
		OperatorRequestRoute::AccountSelect => {
			let selector = operator_account_request_selector(request)?;
			let response = accounts::hydrate_account_list_usage(
				accounts::account_select(&selector)?,
			);

			serde_json::to_vec(&response).map_err(Into::into)
		},
		OperatorRequestRoute::AccountClear => {
			let response =
				accounts::hydrate_account_list_usage(accounts::account_clear()?);

			serde_json::to_vec(&response).map_err(Into::into)
		},
		OperatorRequestRoute::AccountLogout => {
			let selector = operator_account_request_selector(request)?;
			let response = accounts::hydrate_account_list_usage(
				accounts::account_logout(&selector)?,
			);

			serde_json::to_vec(&response).map_err(Into::into)
		},
		OperatorRequestRoute::AccountImport => {
			let body = operator_account_request_body(request)?;
			let auth_json_path = body
				.auth_json_path
				.as_deref()
				.filter(|path| !path.trim().is_empty())
				.ok_or_else(|| eyre::eyre!("Account import requires auth_json_path."))?;
			let response = accounts::hydrate_account_list_usage(
				accounts::account_import(Path::new(auth_json_path))?,
			);

			serde_json::to_vec(&response).map_err(Into::into)
		},
		OperatorRequestRoute::AccountUse => {
			let body = operator_account_request_body(request)?;
			let selector = body
				.selector
				.as_deref()
				.filter(|selector| !selector.trim().is_empty())
				.ok_or_else(|| eyre::eyre!("Account use requires selector."))?;
			let auth_json_path = body.auth_json_path.as_deref().map(PathBuf::from);
			let response = accounts::account_use(&AccountUseRequest {
				selector: selector.to_owned(),
				auth_json_path,
				json: true,
			})?;

			serde_json::to_vec(&response).map_err(Into::into)
		},
		OperatorRequestRoute::AccountRerollName => {
			let body = operator_account_request_body(request)?;
			let selector = body
				.selector
				.as_deref()
				.filter(|selector| !selector.trim().is_empty())
				.ok_or_else(|| eyre::eyre!("Account name reroll requires selector."))?;
			let response = accounts::hydrate_account_list_usage(accounts::account_reroll_name(
				selector,
				body.random_name_offset,
			)?);

			serde_json::to_vec(&response).map_err(Into::into)
		},
		_ => eyre::bail!("Unsupported account API route."),
	}
}

fn operator_account_request_selector(request: &[u8]) -> Result<String> {
	let body = operator_account_request_body(request)?;

	body.selector
		.filter(|selector| !selector.trim().is_empty())
		.ok_or_else(|| eyre::eyre!("Account request requires selector."))
}

fn operator_account_request_body(request: &[u8]) -> Result<OperatorAccountRequest> {
	let body = operator_http_request_body(request)?;

	if body.is_empty() {
		return Ok(OperatorAccountRequest {
			selector: None,
			auth_json_path: None,
			random_name_offset: None,
		});
	}

	serde_json::from_slice(body)
		.map_err(|error| eyre::eyre!("Account request body was not valid JSON: {error}"))
}

fn operator_http_request_body(request: &[u8]) -> Result<&[u8]> {
	let body_offset = request
		.windows(OPERATOR_STATE_HEADER_TERMINATOR.len())
		.position(|window| window == OPERATOR_STATE_HEADER_TERMINATOR)
		.map(|index| index + OPERATOR_STATE_HEADER_TERMINATOR.len())
		.ok_or_else(|| eyre::eyre!("Operator HTTP request omitted header terminator."))?;

	Ok(&request[body_offset..])
}

fn build_operator_state_http_response_for_route(route: OperatorRequestRoute) -> Vec<u8> {
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
		OperatorRequestRoute::AppSnapshot => {
			http_response_bytes("200 OK", "application/json", b"{}")
		},
		OperatorRequestRoute::Live => {
			http_response_bytes("200 OK", "text/plain; charset=utf-8", b"ok")
		},
		OperatorRequestRoute::AccountList { .. }
		| OperatorRequestRoute::AccountSelect
		| OperatorRequestRoute::AccountClear
		| OperatorRequestRoute::AccountLogout
		| OperatorRequestRoute::AccountImport
		| OperatorRequestRoute::AccountUse
		| OperatorRequestRoute::AccountRerollName =>
			http_response_bytes("405 Method Not Allowed", "text/plain; charset=utf-8", b"method not allowed"),
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
	let path_without_query = path
		.split_once('?')
		.map_or(path, |(path_without_query, _)| path_without_query);
	let query = path.split_once('?').map(|(_, query)| query).unwrap_or_default();
	let normalized_path = path_without_query
		.split_once('#')
		.map_or(path_without_query, |(path_without_fragment, _)| path_without_fragment);

	match (method, normalized_path) {
		("GET", OPERATOR_DASHBOARD_ENDPOINT_PATH | OPERATOR_DASHBOARD_ALIAS_ENDPOINT_PATH) =>
			Ok(OperatorRequestRoute::Dashboard),
		("GET", "/assets/icon.png") => Ok(OperatorRequestRoute::DashboardIconPng),
		("GET", "/assets/logo.ico") => Ok(OperatorRequestRoute::DashboardLogoIco),
		("GET", "/assets/logo-touch.png") => Ok(OperatorRequestRoute::DashboardLogoTouchPng),
		("GET", OPERATOR_DASHBOARD_WS_ENDPOINT_PATH) => Ok(OperatorRequestRoute::DashboardWs),
		("GET", OPERATOR_LIVE_ENDPOINT_PATH) => Ok(OperatorRequestRoute::Live),
		("GET", OPERATOR_APP_SNAPSHOT_ENDPOINT_PATH) => Ok(OperatorRequestRoute::AppSnapshot),
		("GET", OPERATOR_ACCOUNTS_ENDPOINT_PATH) => Ok(OperatorRequestRoute::AccountList {
			force_refresh: operator_query_has_flag(query, "refresh"),
		}),
		("POST", "/api/accounts/select") => Ok(OperatorRequestRoute::AccountSelect),
		("POST", "/api/accounts/clear") => Ok(OperatorRequestRoute::AccountClear),
		("POST", "/api/accounts/logout") => Ok(OperatorRequestRoute::AccountLogout),
		("POST", "/api/accounts/import") => Ok(OperatorRequestRoute::AccountImport),
		("POST", "/api/accounts/use") => Ok(OperatorRequestRoute::AccountUse),
		("POST", "/api/accounts/reroll-name") => Ok(OperatorRequestRoute::AccountRerollName),
		(_, OPERATOR_DASHBOARD_ENDPOINT_PATH
			| OPERATOR_DASHBOARD_ALIAS_ENDPOINT_PATH
			| OPERATOR_DASHBOARD_WS_ENDPOINT_PATH
			| OPERATOR_LIVE_ENDPOINT_PATH
			| OPERATOR_APP_SNAPSHOT_ENDPOINT_PATH
			| OPERATOR_ACCOUNTS_ENDPOINT_PATH
			| "/api/accounts/select"
			| "/api/accounts/clear"
			| "/api/accounts/logout"
			| "/api/accounts/import"
			| "/api/accounts/use"
			| "/api/accounts/reroll-name") => Err(http_response_bytes(
			"405 Method Not Allowed",
			"text/plain; charset=utf-8",
			b"method not allowed",
		)),
		_ => Err(http_response_bytes("404 Not Found", "text/plain; charset=utf-8", b"not found")),
	}
}

fn operator_query_has_flag(query: &str, name: &str) -> bool {
	query.split('&').any(|part| {
		let key = part.split_once('=').map_or(part, |(key, _)| key);

		key == name
	})
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
