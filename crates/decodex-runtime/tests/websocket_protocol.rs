//! End-to-end loopback WebSocket protocol acceptance tests.
#![allow(unused_crate_dependencies)]

use std::{
	future::{self, Future},
	net::{Ipv4Addr, SocketAddr},
	sync::{Arc, Mutex},
	time::Duration,
};

use futures_util::{SinkExt as _, StreamExt as _};
use tokio::{net::TcpStream, time};
use tokio_tungstenite::{self, MaybeTlsStream, WebSocketStream, tungstenite::Message};

use decodex_protocol::{
	CURRENT_VERSION, CausationId, Channel, ClientCommandId, ClientHello, ClientMessage,
	CommandEnvelope, CommandError, CommandPayload, CorrelationId, Cursor, EntityId, EntityRevision,
	EventPayload, IdempotencyKey, PREVIOUS_MINOR_VERSION, ProtocolVersion, ReceiptDisposition,
	ReconnectMode, Refusal, ResultPayload, ResumeCursor, ServerId, ServerMessage, SnapshotItem,
	VersionRefusal, WireText,
};
use decodex_runtime::{Application, ApplicationPublication, ProtocolServer, ServerConfig};

type Client = WebSocketStream<MaybeTlsStream<TcpStream>>;

#[derive(Clone, Default)]
struct FixtureApplication {
	state: Arc<Mutex<FixtureState>>,
	execution_delay: Duration,
}
impl FixtureApplication {
	fn executions(&self) -> u64 {
		self.state.lock().expect("test state mutex poisoned").executions
	}

	fn with_status(status: WireText) -> Self {
		Self {
			state: Arc::new(Mutex::new(FixtureState { status, ..FixtureState::default() })),
			..Self::default()
		}
	}

	fn with_delay(execution_delay: Duration) -> Self {
		Self { execution_delay, ..Self::default() }
	}
}

impl Application for FixtureApplication {
	fn snapshot(&self) -> impl Future<Output = Vec<SnapshotItem>> + Send {
		let state = self.state.lock().expect("test state mutex poisoned");

		future::ready(vec![SnapshotItem::SystemState {
			entity_id: EntityId::new("fixture-system").expect("bounded fixture ID"),
			revision: EntityRevision(state.revision),
			status: state.status.clone(),
		}])
	}

	async fn execute<'a>(
		&'a self,
		command: &'a CommandEnvelope,
	) -> Result<ApplicationPublication, CommandError> {
		if !self.execution_delay.is_zero() {
			time::sleep(self.execution_delay).await;
		}

		let CommandPayload::RefreshSystemObservation { entity_id } = &command.payload;
		let mut state = self.state.lock().expect("test state mutex poisoned");

		if let Some(expected) = command.expected_revision
			&& expected != EntityRevision(state.revision)
		{
			return Err(CommandError::ExpectedRevisionMismatch {
				expected,
				actual: EntityRevision(state.revision),
			});
		}

		state.revision += 1;
		state.executions += 1;
		state.status = WireText::new(format!("observation-{}", state.executions))
			.expect("fixture status is bounded");

		Ok(ApplicationPublication {
			channel: Channel::SystemHealth,
			entity_id: entity_id.clone(),
			entity_revision: EntityRevision(state.revision),
			result: ResultPayload::SystemObservationRefreshed { status: state.status.clone() },
			event: EventPayload::SystemObservationRefreshed { status: state.status.clone() },
		})
	}
}

struct FixtureState {
	revision: u64,
	executions: u64,
	status: WireText,
}
impl Default for FixtureState {
	fn default() -> Self {
		Self {
			revision: 0,
			executions: 0,
			status: WireText::new("").expect("empty fixture status is bounded"),
		}
	}
}

fn server(
	id: &str,
	application: FixtureApplication,
	config: ServerConfig,
) -> ProtocolServer<FixtureApplication> {
	ProtocolServer::new(ServerId::new(id).expect("bounded fixture server ID"), application, config)
}

fn command(version: ProtocolVersion, number: u64, key: &str) -> ClientMessage {
	ClientMessage::Command(CommandEnvelope {
		version,
		client_command_id: ClientCommandId::new(format!("command-{number}"))
			.expect("bounded fixture command ID"),
		idempotency_key: IdempotencyKey::new(key).expect("bounded fixture key"),
		expected_revision: None,
		correlation_id: CorrelationId::new(format!("correlation-{number}"))
			.expect("bounded fixture correlation ID"),
		causation_id: Some(
			CausationId::new(format!("cause-{number}")).expect("bounded fixture causation ID"),
		),
		payload: CommandPayload::RefreshSystemObservation {
			entity_id: EntityId::new("fixture-system").expect("bounded fixture entity ID"),
		},
	})
}

async fn connect(address: SocketAddr, version: ProtocolVersion) -> Client {
	let (mut client, _) = tokio_tungstenite::connect_async(format!("ws://{address}/v1/ws"))
		.await
		.expect("connect real WebSocket client");

	send(&mut client, ClientMessage::Hello(ClientHello { version, resume: None })).await;

	client
}

async fn reconnect(address: SocketAddr, version: ProtocolVersion, cursor: ResumeCursor) -> Client {
	let (mut client, _) = tokio_tungstenite::connect_async(format!("ws://{address}/v1/ws"))
		.await
		.expect("connect real slow WebSocket client");

	send(&mut client, ClientMessage::Hello(ClientHello { version, resume: Some(cursor) })).await;

	client
}

async fn send(client: &mut Client, message: ClientMessage) {
	let encoded = serde_json::to_string(&message).expect("serialize client message");

	client.send(Message::Text(encoded.into())).await.expect("send client message");
}

async fn receive(client: &mut Client) -> ServerMessage {
	let message = time::timeout(Duration::from_secs(3), client.next())
		.await
		.expect("server response timed out")
		.expect("server closed before response")
		.expect("websocket response failed");
	let Message::Text(text) = message else {
		panic!("expected text response, got {message:?}");
	};

	serde_json::from_str(&text).expect("decode typed server message")
}

async fn receive_initial(client: &mut Client) -> (ServerId, Cursor, ReconnectMode) {
	let ServerMessage::Welcome(welcome) = receive(client).await else {
		panic!("expected welcome");
	};

	assert!(matches!(receive(client).await, ServerMessage::Snapshot(_)));

	(welcome.server_id, welcome.cursor, welcome.reconnect)
}

async fn execute_and_receive_event(
	client: &mut Client,
	version: ProtocolVersion,
	number: u64,
) -> Cursor {
	send(client, command(version, number, &format!("key-{number}"))).await;

	assert!(matches!(receive(client).await, ServerMessage::CommandReceipt(_)));
	assert!(matches!(receive(client).await, ServerMessage::CommandResult(_)));

	let ServerMessage::Event(event) = receive(client).await else {
		panic!("expected event");
	};

	assert_eq!(event.version, version);
	assert_eq!(event.channel, Channel::SystemHealth);
	assert_eq!(
		event.entity_id,
		EntityId::new("fixture-system").expect("bounded fixture entity ID")
	);
	assert_eq!(event.entity_revision, EntityRevision(number));
	assert_eq!(
		event.correlation_id,
		CorrelationId::new(format!("correlation-{number}"))
			.expect("bounded fixture correlation ID")
	);
	assert_eq!(
		event.causation_id,
		Some(CausationId::new(format!("cause-{number}")).expect("bounded fixture causation ID"))
	);

	event.cursor
}

#[tokio::test]
async fn current_previous_minor_and_exact_major_refusal_use_real_websockets() {
	let bound = server("version-server", FixtureApplication::default(), ServerConfig::default())
		.bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
		.await
		.unwrap();

	for (index, version) in [PREVIOUS_MINOR_VERSION, CURRENT_VERSION].into_iter().enumerate() {
		let mut client = connect(bound.address(), version).await;
		let ServerMessage::Welcome(welcome) = receive(&mut client).await else {
			panic!("expected welcome");
		};

		assert_eq!(welcome.version, version);
		assert!(matches!(receive(&mut client).await, ServerMessage::Snapshot(_)));

		execute_and_receive_event(&mut client, version, index as u64 + 1).await;

		client.close(None).await.unwrap();
	}

	let mut client = connect(bound.address(), ProtocolVersion { major: 2, minor: 0 }).await;
	let ServerMessage::Refusal(refusal) = receive(&mut client).await else {
		panic!("expected version refusal");
	};

	assert!(matches!(
		refusal.refusal,
		Refusal::UnsupportedVersion(VersionRefusal::MajorMismatch { .. })
	));

	drop(client);

	let mut client = connect(bound.address(), ProtocolVersion { major: 1, minor: 9 }).await;
	let ServerMessage::Refusal(refusal) = receive(&mut client).await else {
		panic!("expected minor-version refusal");
	};

	assert!(matches!(
		refusal.refusal,
		Refusal::UnsupportedVersion(VersionRefusal::UnsupportedMinor { .. })
	));

	drop(client);

	bound.shutdown().await.unwrap();
}

#[tokio::test]
async fn duplicate_command_returns_the_original_receipt_and_mutates_once() {
	let application = FixtureApplication::default();
	let bound = server("idempotency-server", application.clone(), ServerConfig::default())
		.bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
		.await
		.unwrap();
	let mut client = connect(bound.address(), CURRENT_VERSION).await;

	receive_initial(&mut client).await;
	send(&mut client, command(CURRENT_VERSION, 1, "same-key")).await;

	let ServerMessage::CommandReceipt(first) = receive(&mut client).await else {
		panic!("expected receipt");
	};

	assert_eq!(first.disposition, ReceiptDisposition::Executed);

	let first_result = receive(&mut client).await;
	let first_event = receive(&mut client).await;

	assert!(matches!(first_result, ServerMessage::CommandResult(_)));

	let ServerMessage::Event(first_event) = first_event else {
		panic!("expected first event");
	};
	let server_id = first_event.server_id.clone();
	let cursor = first_event.cursor;

	drop(client);

	let mut client =
		reconnect(bound.address(), CURRENT_VERSION, ResumeCursor { server_id, cursor }).await;
	let ServerMessage::Welcome(welcome) = receive(&mut client).await else {
		panic!("expected reconnect welcome");
	};

	assert_eq!(welcome.reconnect, ReconnectMode::Resume);

	send(&mut client, command(CURRENT_VERSION, 2, "same-key")).await;

	let ServerMessage::CommandReceipt(receipt) = receive(&mut client).await else {
		panic!("expected duplicate receipt");
	};

	assert_eq!(receipt.disposition, ReceiptDisposition::Duplicate);
	assert_eq!(
		receipt.original_client_command_id,
		ClientCommandId::new("command-1").expect("bounded fixture command ID")
	);

	let ServerMessage::CommandResult(result) = receive(&mut client).await else {
		panic!("expected duplicate result readback");
	};

	assert_eq!(
		result.client_command_id,
		ClientCommandId::new("command-2").expect("bounded fixture command ID")
	);
	assert_eq!(application.executions(), 1);
	assert!(time::timeout(Duration::from_millis(100), client.next()).await.is_err());

	drop(client);

	bound.shutdown().await.unwrap();
}

#[tokio::test]
async fn expected_revision_mismatch_is_rejected_without_publication() {
	let application = FixtureApplication::default();
	let bound = server("revision-server", application.clone(), ServerConfig::default())
		.bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
		.await
		.unwrap();
	let mut client = connect(bound.address(), CURRENT_VERSION).await;

	receive_initial(&mut client).await;

	let mut message = command(CURRENT_VERSION, 1, "revision-key");
	let ClientMessage::Command(ref mut command) = message else { unreachable!() };

	command.expected_revision = Some(EntityRevision(7));

	send(&mut client, message).await;

	assert!(matches!(receive(&mut client).await, ServerMessage::CommandReceipt(_)));

	let ServerMessage::CommandResult(result) = receive(&mut client).await else {
		panic!("expected rejected result");
	};

	assert_eq!(result.outcome, decodex_protocol::CommandOutcome::Rejected);
	assert!(matches!(
		result.error,
		Some(CommandError::ExpectedRevisionMismatch {
			expected: EntityRevision(7),
			actual: EntityRevision(0),
		})
	));
	assert_eq!(application.executions(), 0);
	assert!(time::timeout(Duration::from_millis(100), client.next()).await.is_err());

	drop(client);

	bound.shutdown().await.unwrap();
}

#[tokio::test]
async fn reconnect_resumes_ordered_deltas_and_falls_back_to_a_snapshot() {
	let config = ServerConfig { replay_capacity: 2, ..ServerConfig::default() };
	let bound = server("resume-server", FixtureApplication::default(), config)
		.bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
		.await
		.unwrap();
	let mut first = connect(bound.address(), CURRENT_VERSION).await;
	let (server_id, _, _) = receive_initial(&mut first).await;
	let persisted = execute_and_receive_event(&mut first, CURRENT_VERSION, 1).await;

	execute_and_receive_event(&mut first, CURRENT_VERSION, 2).await;
	execute_and_receive_event(&mut first, CURRENT_VERSION, 3).await;
	drop(first);

	let mut resumed = reconnect(
		bound.address(),
		CURRENT_VERSION,
		ResumeCursor { server_id: server_id.clone(), cursor: persisted },
	)
	.await;
	let ServerMessage::Welcome(welcome) = receive(&mut resumed).await else {
		panic!("expected resume welcome");
	};

	assert_eq!(welcome.reconnect, ReconnectMode::Resume);
	assert_eq!(welcome.cursor, Cursor(3));

	drop(resumed);

	let mut resumed = reconnect(
		bound.address(),
		CURRENT_VERSION,
		ResumeCursor { server_id: server_id.clone(), cursor: persisted },
	)
	.await;
	let ServerMessage::Welcome(welcome) = receive(&mut resumed).await else {
		panic!("expected repeated resume welcome");
	};

	assert_eq!(welcome.reconnect, ReconnectMode::Resume);

	let ServerMessage::Event(second) = receive(&mut resumed).await else {
		panic!("expected second event");
	};
	let ServerMessage::Event(third) = receive(&mut resumed).await else {
		panic!("expected third event");
	};

	assert_eq!((second.cursor, third.cursor), (Cursor(2), Cursor(3)));

	drop(resumed);

	let mut stale =
		reconnect(bound.address(), CURRENT_VERSION, ResumeCursor { server_id, cursor: Cursor(0) })
			.await;
	let ServerMessage::Welcome(welcome) = receive(&mut stale).await else {
		panic!("expected fallback welcome");
	};

	assert_eq!(welcome.reconnect, ReconnectMode::SnapshotFallback);

	let ServerMessage::Snapshot(snapshot) = receive(&mut stale).await else {
		panic!("expected fallback snapshot");
	};

	assert_eq!(snapshot.cursor, Cursor(3));

	drop(stale);

	bound.shutdown().await.unwrap();
}

#[tokio::test]
async fn bounded_outbound_queue_disconnects_a_slow_real_client() {
	let config = ServerConfig { outbound_queue_capacity: 1, ..ServerConfig::default() };
	let application = FixtureApplication::default();
	let bound = server("backpressure-server", application.clone(), config)
		.bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
		.await
		.unwrap();
	let mut client = connect(bound.address(), CURRENT_VERSION).await;
	let (server_id, cursor, _) = receive_initial(&mut client).await;

	send(&mut client, command(CURRENT_VERSION, 1, "slow-1")).await;

	let close_code = time::timeout(Duration::from_secs(3), async {
		while let Some(message) = client.next().await {
			if let Ok(Message::Close(frame)) = message {
				return frame.map(|frame| frame.code);
			}
		}

		None
	})
	.await
	.unwrap();

	assert_eq!(
		close_code,
		Some(tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::Again)
	);
	assert_eq!(application.executions(), 1);

	drop(client);

	let mut resumed =
		reconnect(bound.address(), CURRENT_VERSION, ResumeCursor { server_id, cursor }).await;
	let ServerMessage::Welcome(welcome) = receive(&mut resumed).await else {
		panic!("expected resume welcome after initiator overflow");
	};

	assert_eq!(welcome.reconnect, ReconnectMode::Resume);

	let ServerMessage::Event(event) = receive(&mut resumed).await else {
		panic!("expected retained event after initiator overflow");
	};

	assert_eq!(event.cursor, Cursor(1));

	drop(resumed);

	bound.shutdown().await.unwrap();
}

#[tokio::test]
async fn event_overflow_stops_pipelined_commands_and_retains_the_first_event() {
	let config = ServerConfig { outbound_queue_capacity: 2, ..ServerConfig::default() };
	let application = FixtureApplication::default();
	let bound = server("event-overflow", application.clone(), config)
		.bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
		.await
		.expect("bind event-overflow server");
	let mut client = connect(bound.address(), CURRENT_VERSION).await;
	let (server_id, cursor, _) = receive_initial(&mut client).await;

	send(&mut client, command(CURRENT_VERSION, 1, "overflow-first")).await;
	send(&mut client, command(CURRENT_VERSION, 2, "must-not-execute")).await;

	let close_code = time::timeout(Duration::from_secs(3), async {
		while let Some(message) = client.next().await {
			if let Ok(Message::Close(frame)) = message {
				return frame.map(|frame| frame.code);
			}
		}

		None
	})
	.await
	.expect("event overflow did not close");

	assert_eq!(
		close_code,
		Some(tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::Again)
	);
	assert_eq!(application.executions(), 1);

	drop(client);

	let mut resumed =
		reconnect(bound.address(), CURRENT_VERSION, ResumeCursor { server_id, cursor }).await;

	assert!(matches!(receive(&mut resumed).await, ServerMessage::Welcome(_)));

	let ServerMessage::Event(event) = receive(&mut resumed).await else {
		panic!("expected retained first event after overflow");
	};

	assert_eq!(event.cursor, Cursor(1));
	assert_eq!(application.executions(), 1);

	drop(resumed);

	bound.shutdown().await.expect("shutdown event-overflow server");
}

#[tokio::test]
async fn disconnected_delayed_command_finishes_and_deduplicates_on_reconnect() {
	let application = FixtureApplication::with_delay(Duration::from_millis(200));
	let bound = server("disconnect-shield", application.clone(), ServerConfig::default())
		.bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
		.await
		.expect("bind disconnect-shield server");
	let mut first = connect(bound.address(), CURRENT_VERSION).await;

	receive_initial(&mut first).await;
	send(&mut first, command(CURRENT_VERSION, 1, "disconnect-key")).await;

	time::sleep(Duration::from_millis(25)).await;

	drop(first);

	time::sleep(Duration::from_millis(300)).await;

	assert_eq!(application.executions(), 1);

	let mut reconnected = connect(bound.address(), CURRENT_VERSION).await;

	receive_initial(&mut reconnected).await;
	send(&mut reconnected, command(CURRENT_VERSION, 2, "disconnect-key")).await;

	let ServerMessage::CommandReceipt(receipt) = receive(&mut reconnected).await else {
		panic!("expected duplicate receipt after disconnect");
	};

	assert_eq!(receipt.disposition, ReceiptDisposition::Duplicate);
	assert!(matches!(receive(&mut reconnected).await, ServerMessage::CommandResult(_)));
	assert_eq!(application.executions(), 1);

	drop(reconnected);

	bound.shutdown().await.expect("shutdown disconnect-shield server");
}

#[tokio::test]
async fn full_idempotency_ledger_keeps_duplicates_and_refuses_new_keys() {
	let config = ServerConfig { receipt_capacity: 1, ..ServerConfig::default() };
	let application = FixtureApplication::default();
	let bound = server("receipt-capacity", application.clone(), config)
		.bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
		.await
		.expect("bind receipt-capacity server");
	let mut client = connect(bound.address(), CURRENT_VERSION).await;

	receive_initial(&mut client).await;
	execute_and_receive_event(&mut client, CURRENT_VERSION, 1).await;
	send(&mut client, command(CURRENT_VERSION, 2, "key-1")).await;

	let ServerMessage::CommandReceipt(duplicate) = receive(&mut client).await else {
		panic!("expected duplicate receipt at capacity");
	};

	assert_eq!(duplicate.disposition, ReceiptDisposition::Duplicate);
	assert!(matches!(receive(&mut client).await, ServerMessage::CommandResult(_)));

	send(&mut client, command(CURRENT_VERSION, 3, "new-key-at-capacity")).await;

	let ServerMessage::CommandReceipt(refused) = receive(&mut client).await else {
		panic!("expected capacity refusal receipt");
	};
	let ServerMessage::CommandResult(result) = receive(&mut client).await else {
		panic!("expected capacity refusal result");
	};

	assert_eq!(refused.disposition, ReceiptDisposition::Refused);
	assert!(matches!(
		result.error,
		Some(CommandError::IdempotencyCapacityExceeded { capacity: 1 })
	));
	assert_eq!(application.executions(), 1);
	assert!(time::timeout(Duration::from_millis(100), client.next()).await.is_err());

	drop(client);

	bound.shutdown().await.expect("shutdown receipt-capacity server");
}

#[tokio::test]
async fn oversized_outbound_snapshot_is_closed_before_transmission() {
	let config = ServerConfig { maximum_message_bytes: 512, ..ServerConfig::default() };
	let application = FixtureApplication::with_status(
		WireText::new("x".repeat(1_024)).expect("fixture remains below scalar limit"),
	);
	let bound = server("snapshot-size", application, config)
		.bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
		.await
		.expect("bind snapshot-size server");
	let mut client = connect(bound.address(), CURRENT_VERSION).await;

	assert!(matches!(receive(&mut client).await, ServerMessage::Welcome(_)));

	let close = time::timeout(Duration::from_secs(3), client.next())
		.await
		.expect("oversized outbound snapshot did not close")
		.expect("server ended without close frame")
		.expect("websocket close read failed");
	let Message::Close(Some(frame)) = close else {
		panic!("expected close frame for oversized snapshot, got {close:?}");
	};

	assert_eq!(
		frame.code,
		tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::Size
	);

	drop(client);

	bound.shutdown().await.expect("shutdown snapshot-size server");
}

#[tokio::test]
async fn oversized_wire_identifier_is_refused_without_execution() {
	let application = FixtureApplication::default();
	let bound = server("bounded-identifiers", application.clone(), ServerConfig::default())
		.bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
		.await
		.expect("bind bounded-identifiers server");
	let mut client = connect(bound.address(), CURRENT_VERSION).await;

	receive_initial(&mut client).await;

	let raw = serde_json::json!({
		"type": "command",
		"body": {
			"version": { "major": 1, "minor": 1 },
			"client_command_id": "x".repeat(decodex_protocol::MAX_WIRE_TEXT_BYTES + 1),
			"idempotency_key": "bounded-key",
			"expected_revision": null,
			"correlation_id": "bounded-correlation",
			"causation_id": null,
			"payload": {
				"name": "refresh_system_observation",
				"arguments": { "entity_id": "fixture-system" }
			}
		}
	})
	.to_string();

	client.send(Message::Text(raw.into())).await.expect("send oversized identifier command");

	let ServerMessage::Refusal(refusal) = receive(&mut client).await else {
		panic!("expected protocol refusal for oversized identifier");
	};

	assert!(matches!(refusal.refusal, Refusal::ProtocolViolation { .. }));
	assert_eq!(application.executions(), 0);

	drop(client);

	bound.shutdown().await.expect("shutdown bounded-identifiers server");
}

#[tokio::test]
async fn restart_changes_server_identity_and_forces_snapshot_fallback() {
	let application = FixtureApplication::default();
	let first = server("before-restart", application.clone(), ServerConfig::default())
		.bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
		.await
		.unwrap();
	let address = first.address();
	let mut client = connect(address, CURRENT_VERSION).await;
	let (server_id, _, _) = receive_initial(&mut client).await;
	let cursor = execute_and_receive_event(&mut client, CURRENT_VERSION, 1).await;

	drop(client);

	first.shutdown().await.unwrap();

	let restarted =
		server("after-restart", application, ServerConfig::default()).bind(address).await.unwrap();
	let mut client =
		reconnect(restarted.address(), CURRENT_VERSION, ResumeCursor { server_id, cursor }).await;
	let ServerMessage::Welcome(welcome) = receive(&mut client).await else {
		panic!("expected restart welcome");
	};

	assert_eq!(
		welcome.server_id,
		ServerId::new("after-restart").expect("bounded fixture server ID")
	);
	assert_eq!(welcome.reconnect, ReconnectMode::SnapshotFallback);
	assert!(matches!(receive(&mut client).await, ServerMessage::Snapshot(_)));

	drop(client);

	restarted.shutdown().await.unwrap();
}

#[tokio::test]
async fn non_loopback_binding_is_refused_before_opening_a_socket() {
	let result = server("remote-refusal", FixtureApplication::default(), ServerConfig::default())
		.bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 49_152)))
		.await;
	let error = match result {
		Ok(_) => panic!("non-loopback bind unexpectedly succeeded"),
		Err(error) => error,
	};

	assert_eq!(error.to_string(), "non-loopback endpoint is disabled: 0.0.0.0:49152");
}
