//! End-to-end same-UID Unix WebSocket protocol acceptance tests.
#![allow(unused_crate_dependencies)]

use std::{
	future::{self, Future},
	sync::{Arc, Mutex},
	time::Duration,
};

use futures_util::{SinkExt as _, StreamExt as _};
use tempfile::TempDir;
use tokio::time;
use tokio_tungstenite::{
	self, WebSocketStream,
	tungstenite::{Message, protocol::frame::coding::CloseCode},
};

use decodex_core::{DecodexRoot, LocalTrustPolicy};
use decodex_protocol::{
	CURRENT_VERSION, CausationId, Channel, ClientCommandId, ClientHello, ClientMessage,
	CommandEnvelope, CommandError, CommandPayload, CorrelationId, Cursor, DoctorCheck,
	DoctorComponent, DoctorIssue, DoctorReport, DoctorStatus, EntityId, EntityRevision,
	EventPayload, IdempotencyKey, LocalTransportAuthority, LocalTransportRefusal,
	LocalTransportStream, PREVIOUS_MINOR_VERSION, ProtocolVersion, QueryEnvelope, QueryId,
	QueryPayload, QueryResultPayload, ReceiptDisposition, ReconnectMode, Refusal, ResultPayload,
	ResumeCursor, ServerId, ServerInstanceId, ServerMessage, SnapshotItem, VersionRefusal, WireText,
};
use decodex_runtime::{Application, ApplicationPublication, ProtocolServer, ServerConfig};

type Client = WebSocketStream<LocalTransportStream>;

const OVERFLOW_STRESS_RUNS: u64 = 16;
// Handshake metadata only. The stream is already admitted by the local authority.
const LOCAL_WEBSOCKET_URI: &str = "ws://localhost/v1/ws";

#[derive(Clone, Default)]
struct FixtureApplication {
	state: Arc<Mutex<FixtureState>>,
	execution_delay: Duration,
}
impl FixtureApplication {
	fn executions(&self) -> u64 {
		self.state.lock().expect("test state mutex poisoned").executions
	}

	fn queries(&self) -> u64 {
		self.state.lock().expect("test state mutex poisoned").queries
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

	fn query<'a>(
		&'a self,
		query: &'a QueryEnvelope,
	) -> impl Future<Output = QueryResultPayload> + Send + 'a {
		let QueryPayload::GetDoctorStatus = query.payload else {
			panic!("test application supports only doctor queries");
		};
		let mut state = self.state.lock().expect("test state mutex poisoned");

		state.queries += 1;

		let database = if state.queries == 1 {
			DoctorStatus::Ready
		} else {
			DoctorStatus::Unavailable(DoctorIssue::DatabaseUnreachable)
		};

		future::ready(QueryResultPayload::DoctorStatus(
			DoctorReport::new(
				ServerId::new("fixture-server").expect("bounded fixture server ID"),
				CURRENT_VERSION,
				vec![DoctorCheck::new(DoctorComponent::Database, database)],
			)
			.expect("empty fixture doctor is bounded"),
		))
	}
}

struct FixtureState {
	revision: u64,
	executions: u64,
	queries: u64,
	status: WireText,
}
impl Default for FixtureState {
	fn default() -> Self {
		Self {
			revision: 0,
			executions: 0,
			queries: 0,
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

fn local_transport() -> (TempDir, LocalTransportAuthority) {
	let temp = TempDir::new().expect("local transport temp");
	let root = DecodexRoot::new(
		temp.path().canonicalize().expect("canonical local transport temp").join(".decodex"),
	)
	.expect("local transport root is safe");
	let paths = root.paths();

	paths.ensure_layout().expect("owner-only local transport layout");

	// SAFETY: `geteuid` has no arguments or failure return.
	let service_owner_uid = unsafe { libc::geteuid() };
	let authority = LocalTransportAuthority::new(
		paths,
		LocalTrustPolicy::SameUid,
		Some(service_owner_uid),
	)
	.expect("same-UID local transport authority");

	(temp, authority)
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

fn doctor_query(number: u64) -> ClientMessage {
	ClientMessage::Query(QueryEnvelope {
		version: CURRENT_VERSION,
		query_id: QueryId::new(format!("query-{number}")).expect("bounded fixture query ID"),
		payload: QueryPayload::GetDoctorStatus,
	})
}

async fn connect(
	transport: &LocalTransportAuthority,
	version: ProtocolVersion,
) -> Client {
	let stream = transport.connect().await.expect("connect admitted local stream");
	let (mut client, _) =
		tokio_tungstenite::client_async_with_config(LOCAL_WEBSOCKET_URI, stream, None)
		.await
		.expect("connect real WebSocket client");

	send(
		&mut client,
		ClientMessage::Hello(ClientHello { version, expected_server_id: None, resume: None }),
	)
	.await;

	client
}

async fn reconnect(
	transport: &LocalTransportAuthority,
	version: ProtocolVersion,
	cursor: ResumeCursor,
) -> Client {
	let stream = transport.connect().await.expect("reconnect admitted local stream");
	let (mut client, _) =
		tokio_tungstenite::client_async_with_config(LOCAL_WEBSOCKET_URI, stream, None)
		.await
		.expect("connect real slow WebSocket client");

	send(
		&mut client,
		ClientMessage::Hello(ClientHello {
			version,
			expected_server_id: None,
			resume: Some(cursor),
		}),
	)
	.await;

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

async fn receive_close_code(client: &mut Client) -> Option<CloseCode> {
	time::timeout(Duration::from_secs(3), async {
		while let Some(message) = client.next().await {
			match message {
				Ok(Message::Close(frame)) => return frame.map(|frame| frame.code),
				Ok(_) => {},
				Err(_) => return None,
			}
		}

		None
	})
	.await
	.expect("overflow did not close the connection")
}

async fn receive_initial(
	client: &mut Client,
) -> (ServerId, Option<ServerInstanceId>, Cursor, ReconnectMode) {
	let ServerMessage::Welcome(welcome) = receive(client).await else {
		panic!("expected welcome");
	};

	assert!(matches!(receive(client).await, ServerMessage::Snapshot(_)));

	(welcome.server_id, welcome.instance_id, welcome.cursor, welcome.reconnect)
}

async fn execute_and_receive_event(
	client: &mut Client,
	version: ProtocolVersion,
	number: u64,
) -> Cursor {
	execute_key_and_receive_event(client, version, number, &format!("key-{number}")).await
}

async fn execute_key_and_receive_event(
	client: &mut Client,
	version: ProtocolVersion,
	number: u64,
	key: &str,
) -> Cursor {
	send(client, command(version, number, key)).await;

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
	let (_temp, transport) = local_transport();
	let mut bound = server("version-server", FixtureApplication::default(), ServerConfig::default())
		.bind(transport.clone())
		.await
		.unwrap();

	for (index, version) in [PREVIOUS_MINOR_VERSION, CURRENT_VERSION].into_iter().enumerate() {
		let mut client = connect(&transport, version).await;
		let ServerMessage::Welcome(welcome) = receive(&mut client).await else {
			panic!("expected welcome");
		};

		assert_eq!(welcome.version, version);
		assert_eq!(welcome.instance_id.is_some(), version == CURRENT_VERSION);
		assert!(matches!(receive(&mut client).await, ServerMessage::Snapshot(_)));

		execute_and_receive_event(&mut client, version, index as u64 + 1).await;

		client.close(None).await.unwrap();
	}

	let mut client = connect(&transport, ProtocolVersion { major: 2, minor: 0 }).await;
	let ServerMessage::Refusal(refusal) = receive(&mut client).await else {
		panic!("expected version refusal");
	};

	assert!(matches!(
		refusal.refusal,
		Refusal::UnsupportedVersion(VersionRefusal::MajorMismatch { .. })
	));

	drop(client);

	let mut client = connect(&transport, ProtocolVersion { major: 1, minor: 9 }).await;
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
	let (_temp, transport) = local_transport();
	let mut bound = server("idempotency-server", application.clone(), ServerConfig::default())
		.bind(transport.clone())
		.await
		.unwrap();
	let mut client = connect(&transport, CURRENT_VERSION).await;
	let (_, instance_id, _, _) = receive_initial(&mut client).await;

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

	let mut client = reconnect(
		&transport,
		CURRENT_VERSION,
		ResumeCursor { server_id, instance_id, cursor },
	)
	.await;
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
	let (_temp, transport) = local_transport();
	let mut bound = server("revision-server", application.clone(), ServerConfig::default())
		.bind(transport.clone())
		.await
		.unwrap();
	let mut client = connect(&transport, CURRENT_VERSION).await;

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
	let (_temp, transport) = local_transport();
	let mut bound = server("resume-server", FixtureApplication::default(), config)
		.bind(transport.clone())
		.await
		.unwrap();
	let mut first = connect(&transport, CURRENT_VERSION).await;
	let (server_id, instance_id, _, _) = receive_initial(&mut first).await;
	let persisted = execute_and_receive_event(&mut first, CURRENT_VERSION, 1).await;

	execute_and_receive_event(&mut first, CURRENT_VERSION, 2).await;
	execute_and_receive_event(&mut first, CURRENT_VERSION, 3).await;
	drop(first);

	let mut resumed = reconnect(
		&transport,
		CURRENT_VERSION,
		ResumeCursor {
			server_id: server_id.clone(),
			instance_id: instance_id.clone(),
			cursor: persisted,
		},
	)
	.await;
	let ServerMessage::Welcome(welcome) = receive(&mut resumed).await else {
		panic!("expected resume welcome");
	};

	assert_eq!(welcome.reconnect, ReconnectMode::Resume);
	assert_eq!(welcome.cursor, Cursor(3));

	drop(resumed);

	let mut resumed = reconnect(
		&transport,
		CURRENT_VERSION,
		ResumeCursor {
			server_id: server_id.clone(),
			instance_id: instance_id.clone(),
			cursor: persisted,
		},
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

	let mut stale = reconnect(
		&transport,
		CURRENT_VERSION,
		ResumeCursor { server_id: server_id.clone(), instance_id, cursor: Cursor(0) },
	)
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

	let mut previous = reconnect(
		&transport,
		PREVIOUS_MINOR_VERSION,
		ResumeCursor { server_id, instance_id: None, cursor: persisted },
	)
	.await;
	let ServerMessage::Welcome(welcome) = receive(&mut previous).await else {
		panic!("expected previous-minor fallback welcome");
	};

	assert_eq!(welcome.instance_id, None);
	assert_eq!(welcome.reconnect, ReconnectMode::SnapshotFallback);
	assert!(matches!(receive(&mut previous).await, ServerMessage::Snapshot(_)));

	drop(previous);

	bound.shutdown().await.unwrap();
}

#[tokio::test]
async fn bounded_outbound_queue_disconnects_a_slow_real_client() {
	for run in 0..OVERFLOW_STRESS_RUNS {
		assert_initiator_overflow(run).await;
	}
}

async fn assert_initiator_overflow(run: u64) {
	let config = ServerConfig { outbound_queue_capacity: 1, ..ServerConfig::default() };
	let application = FixtureApplication::default();
	let server_id = format!("backpressure-server-{run}");
	let (_temp, transport) = local_transport();
	let mut bound = server(&server_id, application.clone(), config)
		.bind(transport.clone())
		.await
		.expect("bind initiator-overflow server");
	let mut client = connect(&transport, CURRENT_VERSION).await;
	let (server_id, instance_id, cursor, _) = receive_initial(&mut client).await;

	send(&mut client, command(CURRENT_VERSION, 1, "slow-1")).await;

	assert_eq!(receive_close_code(&mut client).await, Some(CloseCode::Again));
	assert_eq!(application.executions(), 1);

	drop(client);

	let mut resumed = reconnect(
		&transport,
		CURRENT_VERSION,
		ResumeCursor { server_id, instance_id, cursor },
	)
	.await;
	let ServerMessage::Welcome(welcome) = receive(&mut resumed).await else {
		panic!("expected resume welcome after initiator overflow");
	};

	assert_eq!(welcome.reconnect, ReconnectMode::Resume);

	let ServerMessage::Event(event) = receive(&mut resumed).await else {
		panic!("expected retained event after initiator overflow");
	};

	assert_eq!(event.cursor, Cursor(1));

	drop(resumed);

	bound.shutdown().await.expect("shutdown initiator-overflow server");
}

#[tokio::test]
async fn event_overflow_stops_pipelined_commands_and_retains_the_first_event() {
	for run in 0..OVERFLOW_STRESS_RUNS {
		assert_event_overflow(run).await;
	}
}

async fn assert_event_overflow(run: u64) {
	let config = ServerConfig { outbound_queue_capacity: 2, ..ServerConfig::default() };
	let application = FixtureApplication::default();
	let server_id = format!("event-overflow-{run}");
	let (_temp, transport) = local_transport();
	let mut bound = server(&server_id, application.clone(), config)
		.bind(transport.clone())
		.await
		.expect("bind event-overflow server");
	let mut client = connect(&transport, CURRENT_VERSION).await;
	let (server_id, instance_id, cursor, _) = receive_initial(&mut client).await;

	send(&mut client, command(CURRENT_VERSION, 1, "overflow-first")).await;
	send(&mut client, command(CURRENT_VERSION, 2, "must-not-execute")).await;

	assert_eq!(receive_close_code(&mut client).await, Some(CloseCode::Again));
	assert_eq!(application.executions(), 1);

	drop(client);

	let mut resumed = reconnect(
		&transport,
		CURRENT_VERSION,
		ResumeCursor { server_id, instance_id, cursor },
	)
	.await;

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
	let (_temp, transport) = local_transport();
	let mut bound = server("disconnect-shield", application.clone(), ServerConfig::default())
		.bind(transport.clone())
		.await
		.expect("bind disconnect-shield server");
	let mut first = connect(&transport, CURRENT_VERSION).await;

	receive_initial(&mut first).await;
	send(&mut first, command(CURRENT_VERSION, 1, "disconnect-key")).await;

	time::sleep(Duration::from_millis(25)).await;

	drop(first);

	time::sleep(Duration::from_millis(300)).await;

	assert_eq!(application.executions(), 1);

	let mut reconnected = connect(&transport, CURRENT_VERSION).await;

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
	let (_temp, transport) = local_transport();
	let mut bound = server("receipt-capacity", application.clone(), config)
		.bind(transport.clone())
		.await
		.expect("bind receipt-capacity server");
	let mut client = connect(&transport, CURRENT_VERSION).await;

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
async fn repeated_live_queries_are_fresh_ordered_and_do_not_consume_receipts() {
	let config = ServerConfig { receipt_capacity: 1, ..ServerConfig::default() };
	let application = FixtureApplication::default();
	let (_temp, transport) = local_transport();
	let mut bound = server("live-query", application.clone(), config)
		.bind(transport.clone())
		.await
		.expect("bind live-query server");
	let mut client = connect(&transport, CURRENT_VERSION).await;

	receive_initial(&mut client).await;
	send(&mut client, doctor_query(1)).await;
	send(&mut client, doctor_query(2)).await;

	let ServerMessage::QueryResult(first) = receive(&mut client).await else {
		panic!("expected first live query result");
	};
	let ServerMessage::QueryResult(second) = receive(&mut client).await else {
		panic!("expected second live query result");
	};
	let QueryResultPayload::DoctorStatus(first_report) = first.payload else {
		panic!("expected doctor result");
	};
	let QueryResultPayload::DoctorStatus(second_report) = second.payload else {
		panic!("expected doctor result");
	};

	assert_eq!(first.query_id, QueryId::new("query-1").expect("bounded query ID"));
	assert_eq!(second.query_id, QueryId::new("query-2").expect("bounded query ID"));
	assert_eq!(
		first_report.check(DoctorComponent::Database).expect("database check").status,
		DoctorStatus::Ready
	);
	assert_eq!(
		second_report.check(DoctorComponent::Database).expect("database check").status,
		DoctorStatus::Unavailable(DoctorIssue::DatabaseUnreachable)
	);
	assert_eq!(application.queries(), 2);

	execute_and_receive_event(&mut client, CURRENT_VERSION, 1).await;
	send(&mut client, command(CURRENT_VERSION, 2, "query-independent-capacity")).await;

	let ServerMessage::CommandReceipt(receipt) = receive(&mut client).await else {
		panic!("expected mutation capacity receipt");
	};
	let ServerMessage::CommandResult(result) = receive(&mut client).await else {
		panic!("expected mutation capacity result");
	};

	assert_eq!(receipt.disposition, ReceiptDisposition::Refused);
	assert!(matches!(
		result.error,
		Some(CommandError::IdempotencyCapacityExceeded { capacity: 1 })
	));
	assert_eq!(application.executions(), 1);

	drop(client);

	bound.shutdown().await.expect("shutdown live-query server");
}

#[tokio::test]
async fn independent_connections_can_observe_live_doctor_concurrently() {
	let application = FixtureApplication::default();
	let (_temp, transport) = local_transport();
	let mut bound = server("concurrent-query", application.clone(), ServerConfig::default())
		.bind(transport.clone())
		.await
		.expect("bind concurrent-query server");
	let mut first = connect(&transport, CURRENT_VERSION).await;
	let mut second = connect(&transport, CURRENT_VERSION).await;

	receive_initial(&mut first).await;
	receive_initial(&mut second).await;

	let ((), ()) =
		tokio::join!(send(&mut first, doctor_query(1)), send(&mut second, doctor_query(2)));
	let (first_result, second_result) = tokio::join!(receive(&mut first), receive(&mut second));

	assert!(matches!(first_result, ServerMessage::QueryResult(_)));
	assert!(matches!(second_result, ServerMessage::QueryResult(_)));
	assert_eq!(application.queries(), 2);

	drop((first, second));

	bound.shutdown().await.expect("shutdown concurrent-query server");
}

#[tokio::test]
async fn receipt_capacity_is_independent_in_both_protocol_version_orderings() {
	for (case, first_version, second_version) in [
		("previous-then-current", PREVIOUS_MINOR_VERSION, CURRENT_VERSION),
		("current-then-previous", CURRENT_VERSION, PREVIOUS_MINOR_VERSION),
	] {
		assert_cross_version_receipt_capacity(case, first_version, second_version).await;
	}
}

async fn assert_cross_version_receipt_capacity(
	case: &str,
	first_version: ProtocolVersion,
	second_version: ProtocolVersion,
) {
	let config = ServerConfig { receipt_capacity: 1, ..ServerConfig::default() };
	let application = FixtureApplication::default();
	let (_temp, transport) = local_transport();
	let mut bound = server(case, application.clone(), config)
		.bind(transport.clone())
		.await
		.expect("bind cross-version receipt-capacity server");
	let mut first = connect(&transport, first_version).await;

	receive_initial(&mut first).await;
	execute_and_receive_event(&mut first, first_version, 1).await;
	send(&mut first, command(first_version, 2, "key-1")).await;

	let ServerMessage::CommandReceipt(duplicate) = receive(&mut first).await else {
		panic!("expected same-version duplicate at capacity");
	};

	assert_eq!(duplicate.disposition, ReceiptDisposition::Duplicate);
	assert!(matches!(receive(&mut first).await, ServerMessage::CommandResult(_)));

	let mut conflict_message = command(first_version, 3, "key-1");
	let ClientMessage::Command(ref mut conflict) = conflict_message else { unreachable!() };

	conflict.expected_revision = Some(EntityRevision(999));

	send(&mut first, conflict_message).await;

	let ServerMessage::CommandReceipt(conflict_receipt) = receive(&mut first).await else {
		panic!("expected same-version conflict receipt at capacity");
	};
	let ServerMessage::CommandResult(conflict_result) = receive(&mut first).await else {
		panic!("expected same-version conflict result at capacity");
	};

	assert_eq!(conflict_receipt.disposition, ReceiptDisposition::Duplicate);
	assert!(matches!(conflict_result.error, Some(CommandError::IdempotencyConflict)));
	assert_eq!(application.executions(), 1);

	drop(first);

	let mut second = connect(&transport, second_version).await;

	receive_initial(&mut second).await;
	execute_key_and_receive_event(&mut second, second_version, 2, "key-1").await;
	send(&mut second, command(second_version, 4, "second-version-capacity")).await;

	let ServerMessage::CommandReceipt(refused) = receive(&mut second).await else {
		panic!("expected second-version capacity refusal");
	};
	let ServerMessage::CommandResult(result) = receive(&mut second).await else {
		panic!("expected second-version capacity result");
	};

	assert_eq!(refused.disposition, ReceiptDisposition::Refused);
	assert!(matches!(
		result.error,
		Some(CommandError::IdempotencyCapacityExceeded { capacity: 1 })
	));
	assert_eq!(application.executions(), 2);

	drop(second);

	bound.shutdown().await.expect("shutdown cross-version receipt-capacity server");
}

#[tokio::test]
async fn oversized_outbound_snapshot_is_closed_before_transmission() {
	let config = ServerConfig { maximum_message_bytes: 512, ..ServerConfig::default() };
	let application = FixtureApplication::with_status(
		WireText::new("x".repeat(1_024)).expect("fixture remains below scalar limit"),
	);
	let (_temp, transport) = local_transport();
	let mut bound = server("snapshot-size", application, config)
		.bind(transport.clone())
		.await
		.expect("bind snapshot-size server");
	let mut client = connect(&transport, CURRENT_VERSION).await;

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
	let (_temp, transport) = local_transport();
	let mut bound = server("bounded-identifiers", application.clone(), ServerConfig::default())
		.bind(transport.clone())
		.await
		.expect("bind bounded-identifiers server");
	let mut client = connect(&transport, CURRENT_VERSION).await;

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
async fn restart_with_stable_server_identity_rejects_equal_and_overlapping_old_epoch_cursors() {
	let application = FixtureApplication::default();
	let (_temp, transport) = local_transport();
	let mut first = server("stable-restart", application.clone(), ServerConfig::default())
		.bind(transport.clone())
		.await
		.unwrap();
	let mut client = connect(&transport, CURRENT_VERSION).await;
	let (server_id, old_instance_id, initial_cursor, _) = receive_initial(&mut client).await;
	let overlapping_cursor = execute_and_receive_event(&mut client, CURRENT_VERSION, 1).await;

	drop(client);

	first.shutdown().await.unwrap();

	let mut restarted = server("stable-restart", application, ServerConfig::default())
		.bind(transport.clone())
		.await
		.unwrap();
	let mut client = reconnect(
		&transport,
		CURRENT_VERSION,
		ResumeCursor {
			server_id: server_id.clone(),
			instance_id: old_instance_id.clone(),
			cursor: initial_cursor,
		},
	)
	.await;
	let ServerMessage::Welcome(welcome) = receive(&mut client).await else {
		panic!("expected restart welcome");
	};

	assert_eq!(welcome.server_id, server_id);
	assert_ne!(welcome.instance_id, old_instance_id);
	assert_eq!(welcome.reconnect, ReconnectMode::SnapshotFallback);
	assert!(matches!(receive(&mut client).await, ServerMessage::Snapshot(_)));

	drop(client);

	let mut current = connect(&transport, CURRENT_VERSION).await;

	receive_initial(&mut current).await;

	assert_eq!(
		execute_and_receive_event(&mut current, CURRENT_VERSION, 2).await,
		overlapping_cursor
	);

	drop(current);

	let mut client = reconnect(
		&transport,
		CURRENT_VERSION,
		ResumeCursor { server_id, instance_id: old_instance_id, cursor: overlapping_cursor },
	)
	.await;
	let ServerMessage::Welcome(welcome) = receive(&mut client).await else {
		panic!("expected overlapping-cursor restart welcome");
	};

	assert_eq!(welcome.reconnect, ReconnectMode::SnapshotFallback);
	assert!(matches!(receive(&mut client).await, ServerMessage::Snapshot(_)));

	drop(client);

	restarted.shutdown().await.unwrap();
}

#[tokio::test]
async fn cross_uid_authority_is_refused_before_opening_a_socket() {
	let temp = TempDir::new().expect("remote-refusal temp");
	let root = DecodexRoot::new(
		temp.path().canonicalize().expect("canonical remote-refusal temp").join(".decodex"),
	)
	.expect("remote-refusal root is safe");
	let paths = root.paths();

	paths.ensure_layout().expect("owner-only refusal layout");

	// SAFETY: `geteuid` has no arguments or failure return.
	let service_owner_uid = unsafe { libc::geteuid() };
	let error = LocalTransportAuthority::new(
		paths,
		LocalTrustPolicy::SameUid,
		Some(service_owner_uid ^ 1),
	)
	.unwrap_err();

	assert_eq!(error, LocalTransportRefusal::EffectiveUidMismatch);
}
