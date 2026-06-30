use std::io::Read;

use base64::Engine as _;
use time::OffsetDateTime;

use super::{
	Arc, DASHBOARD_RUN_ACTIVITY_FINGERPRINT_VOLATILE_FIELDS, DashboardBroadcastEvent,
	DashboardClientFrame, DashboardClientMessage, DashboardControlAck, DashboardEventHub,
	DashboardRunActivityEvent, DashboardWebSocketSession, Duration, ErrorKind, Instant, Mutex,
	OPERATOR_DASHBOARD_WS_CLIENT_MESSAGE_MAX_BYTES, OPERATOR_DASHBOARD_WS_HEARTBEAT_INTERVAL,
	OPERATOR_RUN_ACTIVITY_STREAM_INTERVAL, OperatorCodexAccountControlStatus, OperatorRunStatus,
	PublishedOperatorSnapshot, Receiver, RecvTimeoutError, Result, STANDARD, ServiceConfig, Sha1,
	StateStore, TcpStream, Value, WorkflowDocument, Write, accounts,
	build_operator_state_snapshot_without_live_observers, dashboard_current_snapshot_event_payload,
	eyre, global_codex_account_control_status, http_response_bytes_with_headers, json,
	operator_http_header_contains_token, operator_http_header_value,
	operator_snapshot_presentation_value,
};
use crate::orchestrator::DEFAULT_OPERATOR_DASHBOARD_RUN_LIMIT;

use super::types::DashboardClientSubscription;

pub(super) fn handle_operator_dashboard_websocket_connection(
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

pub(crate) fn run_operator_run_activity_websocket_broadcasts(
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

pub(crate) fn build_operator_run_activity_event(
	state_store: &StateStore,
) -> Result<DashboardRunActivityEvent> {
	let now_unix_epoch = OffsetDateTime::now_utc().unix_timestamp();
	let account_control = global_codex_account_control_status();
	let mut current_lanes = Vec::new();

	for registration in state_store.list_projects()? {
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
		let workflow = match WorkflowDocument::from_path(project.workflow_path()) {
			Ok(workflow) => workflow,
			Err(error) => {
				tracing::debug!(
					project_id = project.service_id(),
					workflow_path = %project.workflow_path().display(),
					?error,
					"Skipped dashboard run activity for a project with an unreadable workflow."
				);

				continue;
			},
		};
		let project_snapshot = build_operator_state_snapshot_without_live_observers(
			&project,
			&workflow,
			state_store,
			DEFAULT_OPERATOR_DASHBOARD_RUN_LIMIT,
		)?;

		if project_snapshot.current_lanes.is_empty() {
			continue;
		}

		current_lanes.extend(project_snapshot.current_lanes);
	}

	let fingerprint_payload =
		dashboard_run_activity_fingerprint_payload(&account_control, &current_lanes)?;
	let fingerprint = serde_json::to_vec(&fingerprint_payload)?;
	let presentation = operator_snapshot_presentation_value(&current_lanes)?;
	let payload = json!({
		"emittedAtUnixEpoch": now_unix_epoch,
		"accountControl": &account_control,
		"currentLanes": &current_lanes,
		"currentLanesComplete": true,
		"currentLaneScope": "complete",
		"presentation": &presentation,
	});

	Ok(DashboardRunActivityEvent {
		fingerprint,
		event: DashboardBroadcastEvent { event_type: "runActivity", payload },
	})
}

pub(super) fn dashboard_run_activity_fingerprint_payload(
	account_control: &OperatorCodexAccountControlStatus,
	current_lanes: &[OperatorRunStatus],
) -> Result<Value> {
	let mut fingerprint_payload = json!({
		"accountControl": account_control,
		"currentLanes": current_lanes,
		"currentLanesComplete": true,
		"currentLaneScope": "complete",
		"presentation": operator_snapshot_presentation_value(current_lanes)?,
	});

	strip_dashboard_run_activity_volatile_fields(&mut fingerprint_payload);

	Ok(fingerprint_payload)
}

pub(crate) fn strip_dashboard_run_activity_volatile_fields(value: &mut Value) {
	match value {
		Value::Object(object) => {
			for field in DASHBOARD_RUN_ACTIVITY_FINGERPRINT_VOLATILE_FIELDS {
				object.remove(*field);
			}
			for child in object.values_mut() {
				strip_dashboard_run_activity_volatile_fields(child);
			}
		},
		Value::Array(values) =>
			for child in values {
				strip_dashboard_run_activity_volatile_fields(child);
			},
		_ => {},
	}
}

pub(super) fn dashboard_control_ack_should_push_snapshot(ack: &Value) -> bool {
	ack.get("accepted").and_then(Value::as_bool).unwrap_or(false)
		&& matches!(
			ack.get("action").and_then(Value::as_str),
			Some("selectAccount" | "clearAccountSelection")
		)
}

pub(super) fn dashboard_control_ack_should_push_run_activity(ack: &Value) -> bool {
	ack.get("accepted").and_then(Value::as_bool).unwrap_or(false)
		&& matches!(
			ack.get("action").and_then(Value::as_str),
			Some("subscribe" | "focus" | "clearFocus" | "selectAccount" | "clearAccountSelection")
		)
}

pub(super) fn write_cached_dashboard_run_activity_event(
	stream: &mut TcpStream,
	dashboard_events: &DashboardEventHub,
	subscription: &DashboardClientSubscription,
) {
	match dashboard_events.cached_run_activity_event(subscription) {
		Some(event) if dashboard_run_activity_event_has_current_lanes(&event) => {
			if let Err(error) =
				write_dashboard_websocket_event(stream, event.event_type, &event.payload)
			{
				tracing::warn!(
					?error,
					"Skipped cached dashboard run activity snapshot for a WebSocket client."
				);
			}
		},
		Some(_) | None => {},
	}
}

pub(super) fn dashboard_run_activity_event_has_current_lanes(
	event: &DashboardBroadcastEvent,
) -> bool {
	event.payload.get("currentLanes").and_then(Value::as_array).is_some_and(|runs| !runs.is_empty())
}

pub(super) fn write_dashboard_websocket_event(
	stream: &mut TcpStream,
	event_type: &'static str,
	payload: &Value,
) -> Result<()> {
	stream.write_all(&dashboard_websocket_message(event_type, payload)?)?;

	Ok(())
}

pub(crate) fn dashboard_websocket_message(event_type: &str, payload: &Value) -> Result<Vec<u8>> {
	let message = serde_json::to_vec(&json!({
		"type": event_type,
		"payload": payload,
	}))?;

	websocket_frame(0x1, &message)
}

pub(super) fn websocket_frame(opcode: u8, payload: &[u8]) -> Result<Vec<u8>> {
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

pub(super) fn websocket_ping_frame() -> Vec<u8> {
	vec![0x89, 0]
}

pub(super) fn read_dashboard_websocket_client_frames(
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
			Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
				break;
			},
			Err(error) if error.kind() == ErrorKind::Interrupted => continue,
			Err(error) => return Err(error.into()),
		}
	}

	Ok(frames)
}

pub(super) fn parse_dashboard_websocket_client_frame(
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

pub(super) fn dashboard_control_ready_payload(subscription: &DashboardClientSubscription) -> Value {
	json!({
		"supportedActions": [
			"subscribe",
			"focus",
			"clearFocus",
			"selectAccount",
			"clearAccountSelection",
			"ack"
		],
		"subscription": dashboard_subscription_payload(subscription),
	})
}

pub(super) fn handle_dashboard_client_message(
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

pub(super) fn handle_dashboard_control_action(
	session: &mut DashboardWebSocketSession,
	state_store: &StateStore,
	message: &DashboardClientMessage,
	action: &str,
) -> Value {
	match action {
		"focus" => dashboard_focus_control_ack(session, message, action),
		"clearFocus" | "clearSubscription" =>
			dashboard_clear_focus_control_ack(session, message, action),
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

pub(super) fn dashboard_focus_control_ack(
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

pub(super) fn dashboard_clear_focus_control_ack(
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

pub(super) fn dashboard_account_selection_control_ack(
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

pub(super) fn dashboard_unsupported_control_ack(
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

pub(super) fn dashboard_control_ack_for_message(
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

pub(super) fn dashboard_control_ack_value(ack: DashboardControlAck<'_>) -> Value {
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

pub(super) fn dashboard_subscription_from_message(
	message: &DashboardClientMessage,
) -> DashboardClientSubscription {
	DashboardClientSubscription {
		project_id: dashboard_clean_scope_value(message.project_id.as_deref()),
		issue_id: dashboard_clean_scope_value(message.issue_id.as_deref()),
		run_id: dashboard_clean_scope_value(message.run_id.as_deref()),
	}
}

pub(super) fn dashboard_subscription_payload(subscription: &DashboardClientSubscription) -> Value {
	json!({
		"projectId": subscription.project_id,
		"issueId": subscription.issue_id,
		"runId": subscription.run_id,
	})
}

pub(super) fn dashboard_required_account_selector(
	message: &DashboardClientMessage,
) -> Option<&str> {
	message.account_selector.as_deref().map(str::trim).filter(|value| !value.is_empty())
}

pub(super) fn dashboard_clean_scope_value(value: Option<&str>) -> Option<String> {
	value.map(str::trim).filter(|value| !value.is_empty()).map(str::to_owned)
}

pub(super) fn dashboard_event_for_subscription(
	event: &DashboardBroadcastEvent,
	subscription: &DashboardClientSubscription,
) -> Option<DashboardBroadcastEvent> {
	if event.event_type != "runActivity" || dashboard_subscription_is_empty(subscription) {
		return Some(event.clone());
	}

	let current_lanes =
		event.payload.get("currentLanes").and_then(Value::as_array).map(|runs| {
			runs.iter()
				.filter(|run| dashboard_run_matches_subscription(run, subscription))
				.cloned()
				.collect::<Vec<_>>()
		})?;
	let current_lanes_complete =
		event.payload.get("currentLanesComplete").and_then(Value::as_bool).unwrap_or(true);
	let current_lane_cards = event
		.payload
		.get("presentation")
		.and_then(|presentation| presentation.get("current_lane_cards"))
		.and_then(Value::as_array)
		.map(|cards| {
			cards
				.iter()
				.filter(|card| {
					let run = card.get("run").unwrap_or(card);

					dashboard_run_matches_subscription(run, subscription)
				})
				.cloned()
				.collect::<Vec<_>>()
		});
	let mut payload = event.payload.clone();

	payload["currentLanes"] = Value::Array(current_lanes);
	payload["currentLanesComplete"] = Value::Bool(current_lanes_complete);
	payload["currentLaneScope"] = Value::String(String::from("filtered"));

	if let Some(current_lane_cards) = current_lane_cards
		&& let Some(presentation) = payload.get_mut("presentation").and_then(Value::as_object_mut)
	{
		presentation.insert(String::from("current_lane_cards"), Value::Array(current_lane_cards));
	}

	Some(DashboardBroadcastEvent { event_type: event.event_type, payload })
}

pub(super) fn dashboard_subscription_is_empty(subscription: &DashboardClientSubscription) -> bool {
	subscription.project_id.is_none()
		&& subscription.issue_id.is_none()
		&& subscription.run_id.is_none()
}

pub(super) fn dashboard_run_matches_subscription(
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

pub(super) fn operator_dashboard_websocket_response_headers(
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

pub(super) fn websocket_upgrade_required_response() -> Vec<u8> {
	http_response_bytes_with_headers(
		"426 Upgrade Required",
		"text/plain; charset=utf-8",
		&[("Upgrade", String::from("websocket"))],
		b"websocket upgrade required",
	)
}

pub(super) fn websocket_accept_key(key: &str) -> String {
	const WEBSOCKET_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

	let mut hasher = <Sha1 as sha1::Digest>::new();

	sha1::Digest::update(&mut hasher, key.as_bytes());
	sha1::Digest::update(&mut hasher, WEBSOCKET_GUID.as_bytes());

	STANDARD.encode(sha1::Digest::finalize(hasher))
}
