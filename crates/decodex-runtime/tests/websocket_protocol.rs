//! End-to-end same-UID Unix WebSocket protocol acceptance tests.
#![allow(unused_crate_dependencies)]

use std::{
	fs,
	future::{self, Future},
	os::unix::{fs::PermissionsExt as _, net::UnixListener as StandardUnixListener},
	pin::Pin,
	sync::{Arc, Mutex},
	time::Duration,
};

use futures_util::{SinkExt as _, StreamExt as _};
use tempfile::TempDir;
use tokio::{
	sync::{Notify, watch},
	time,
};
use tokio_tungstenite::{
	self, WebSocketStream,
	tungstenite::{Message, protocol::frame::coding::CloseCode},
};

use decodex_core::{DecodexRoot, LocalTrustPolicy};
use decodex_protocol::{
	AccountLoginInstallMode, AccountLoginMethod, AccountLoginRequest, AccountLoginRequestEnvelope,
	AccountLoginStart, AccountLoginState, AccountLoginStatus, AccountsResult, CURRENT_VERSION,
	CausationId, Channel, ClientCommandId, ClientHello, ClientMessage, CommandEnvelope,
	CommandError, CommandPayload, CorrelationId, Cursor, DoctorCheck, DoctorComponent, DoctorIssue,
	DoctorReport, DoctorStatus, EntityId, EntityRevision, EventPayload, IdempotencyKey,
	LocalTransportAuthority, LocalTransportRefusal, LocalTransportStream, ProtocolVersion,
	QueryEnvelope, QueryId, QueryPayload, QueryResultPayload, ReceiptDisposition, ReconnectMode,
	Refusal, ResetCardDescriptorDto, ResetCardOperationResult, ResultPayload, ResumeCursor,
	ServerId, ServerInstanceId, ServerMessage, SnapshotItem, VersionRefusal, WireText,
};
use decodex_runtime::{
	ActorCommandDeadlineClass, Application, ApplicationPublication, ProtocolServer, ServerConfig,
	ServerError, TerminationPrimary, TerminationReceipt,
};

type Client = WebSocketStream<LocalTransportStream>;

const OVERFLOW_STRESS_RUNS: u64 = 16;
// Handshake metadata only. The stream is already admitted by the local authority.
const LOCAL_WEBSOCKET_URI: &str = "ws://localhost/v1/ws";

#[derive(Clone, Default)]
struct FixtureApplication {
	state: Arc<Mutex<FixtureState>>,
	execution_delay: Duration,
	daemon_service: Option<Arc<FixtureDaemonService>>,
}
impl FixtureApplication {
	fn executions(&self) -> u64 {
		self.state.lock().expect("test state mutex poisoned").executions
	}

	fn queries(&self) -> u64 {
		self.state.lock().expect("test state mutex poisoned").queries
	}

	fn attempts(&self) -> u64 {
		self.state.lock().expect("test state mutex poisoned").attempts
	}

	fn login_requests(&self) -> u64 {
		self.state.lock().expect("test state mutex poisoned").login_requests
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

	fn with_revision(revision: u64) -> Self {
		Self {
			state: Arc::new(Mutex::new(FixtureState { revision, ..FixtureState::default() })),
			..Self::default()
		}
	}

	fn with_acceptance_unknown_once() -> Self {
		Self {
			state: Arc::new(Mutex::new(FixtureState {
				acceptance_unknown_remaining: 1,
				..FixtureState::default()
			})),
			..Self::default()
		}
	}

	fn with_daemon_service(service: Arc<FixtureDaemonService>) -> Self {
		Self { daemon_service: Some(service), ..Self::default() }
	}
}

#[derive(Default)]
struct FixtureDaemonService {
	started: Notify,
	stop_observed: Notify,
	release: Notify,
}

impl Application for FixtureApplication {
	fn daemon_service_tasks(
		&self,
		mut stop: watch::Receiver<bool>,
	) -> Vec<Pin<Box<dyn Future<Output = ()> + Send + 'static>>> {
		let Some(service) = self.daemon_service.clone() else {
			return Vec::new();
		};

		vec![Box::pin(async move {
			service.started.notify_one();
			loop {
				if *stop.borrow_and_update() {
					break;
				}
				if stop.changed().await.is_err() {
					break;
				}
			}
			service.stop_observed.notify_one();
			service.release.notified().await;
		})]
	}

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

		let mut state = self.state.lock().expect("test state mutex poisoned");

		state.attempts += 1;
		if state.acceptance_unknown_remaining > 0 {
			state.acceptance_unknown_remaining -= 1;
			return Err(CommandError::AcceptanceUnknown);
		}

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

		match &command.payload {
			CommandPayload::RefreshSystemObservation { entity_id } => Ok(ApplicationPublication {
				channel: Channel::SystemHealth,
				entity_id: entity_id.clone(),
				entity_revision: EntityRevision(state.revision),
				result: ResultPayload::SystemObservationRefreshed { status: state.status.clone() },
				event: EventPayload::SystemObservationRefreshed { status: state.status.clone() },
			}),
			CommandPayload::ConsumeResetCard { account_id, descriptor } => {
				let operation_state = ResetCardOperationResult::Prepared;

				Ok(ApplicationPublication {
					channel: Channel::AccountsHealth,
					entity_id: account_id.clone(),
					entity_revision: EntityRevision(state.revision),
					result: ResultPayload::ResetCardOperationAccepted {
						account_id: account_id.clone(),
						descriptor: *descriptor,
						state: operation_state,
					},
					event: EventPayload::ResetCardOperationAccepted {
						account_id: account_id.clone(),
						descriptor: *descriptor,
						state: operation_state,
					},
				})
			},
			_ => Err(CommandError::ApplicationUnavailable {
				message: WireText::new("fixture command is unavailable")
					.expect("fixture failure is bounded"),
			}),
		}
	}

	fn query<'a>(
		&'a self,
		query: &'a QueryEnvelope,
	) -> impl Future<Output = QueryResultPayload> + Send + 'a {
		let mut state = self.state.lock().expect("test state mutex poisoned");

		state.queries += 1;
		let result = match &query.payload {
			QueryPayload::GetDoctorStatus => {
				let database = if state.queries == 1 {
					DoctorStatus::Ready
				} else {
					DoctorStatus::Unavailable(DoctorIssue::DatabaseUnreachable)
				};
				QueryResultPayload::DoctorStatus(
					DoctorReport::new(
						ServerId::new("fixture-server").expect("bounded fixture server ID"),
						CURRENT_VERSION,
						vec![DoctorCheck::new(DoctorComponent::ProductStore, database)],
					)
					.expect("empty fixture doctor is bounded"),
				)
			},
			QueryPayload::ListAccounts => QueryResultPayload::Accounts(AccountsResult::Unavailable),
			_ => panic!("test application does not support this query"),
		};

		future::ready(result)
	}

	async fn account_login<'a>(&'a self, request: &'a AccountLoginRequest) -> AccountLoginStatus {
		self.state.lock().expect("test state mutex poisoned").login_requests += 1;
		let state = match request {
			AccountLoginRequest::Start { start } => match start.method {
				AccountLoginMethod::BrowserRedirect => AccountLoginState::OpeningBrowser,
				AccountLoginMethod::DeviceCode => AccountLoginState::RequestingCode,
			},
			AccountLoginRequest::Status { .. } | AccountLoginRequest::Cancel { .. } =>
				AccountLoginState::Cancelled,
		};
		AccountLoginStatus {
			session_id: request.session_id().clone(),
			state,
			prompt: None,
			authorization_url: None,
			failure: None,
			resolved_account_id: None,
		}
	}
}

struct FixtureState {
	revision: u64,
	executions: u64,
	attempts: u64,
	queries: u64,
	login_requests: u64,
	acceptance_unknown_remaining: u64,
	status: WireText,
}
impl Default for FixtureState {
	fn default() -> Self {
		Self {
			revision: 0,
			executions: 0,
			attempts: 0,
			queries: 0,
			login_requests: 0,
			acceptance_unknown_remaining: 0,
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
	let authority =
		LocalTransportAuthority::new(paths, LocalTrustPolicy::SameUid, Some(service_owner_uid))
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
	doctor_query_for(CURRENT_VERSION, number)
}

fn doctor_query_for(version: ProtocolVersion, number: u64) -> ClientMessage {
	ClientMessage::Query(QueryEnvelope {
		version,
		query_id: QueryId::new(format!("query-{number}")).expect("bounded fixture query ID"),
		payload: QueryPayload::GetDoctorStatus,
	})
}

fn account_login_start() -> ClientMessage {
	ClientMessage::AccountLogin(AccountLoginRequestEnvelope {
		version: CURRENT_VERSION,
		request_id: QueryId::new("account-login-request").expect("bounded fixture query ID"),
		request: AccountLoginRequest::Start {
			start: Box::new(AccountLoginStart {
				session_id: EntityId::new("10000000-0000-4000-8000-000000000001")
					.expect("canonical fixture session ID"),
				method: AccountLoginMethod::BrowserRedirect,
				install_mode: AccountLoginInstallMode::Enroll {
					operation_id: EntityId::new("20000000-0000-4000-8000-000000000001")
						.expect("canonical fixture operation ID"),
					account_id: EntityId::new("30000000-0000-4000-8000-000000000001")
						.expect("canonical fixture account ID"),
					enabled: true,
					idempotency_key: IdempotencyKey::new("account-login-install")
						.expect("bounded fixture idempotency key"),
				},
			}),
		},
	})
}

fn reset_card_command(version: ProtocolVersion, number: u64, key: &str) -> ClientMessage {
	ClientMessage::Command(CommandEnvelope {
		version,
		client_command_id: ClientCommandId::new(format!("reset-command-{number}"))
			.expect("bounded fixture command ID"),
		idempotency_key: IdempotencyKey::new(key).expect("bounded fixture key"),
		expected_revision: Some(EntityRevision(number)),
		correlation_id: CorrelationId::new(format!("reset-correlation-{number}"))
			.expect("bounded fixture correlation ID"),
		causation_id: None,
		payload: CommandPayload::ConsumeResetCard {
			account_id: EntityId::new("01234567-89ab-4def-8123-456789abcdef")
				.expect("canonical fixture account ID"),
			descriptor: ResetCardDescriptorDto::new(100, 200).expect("valid descriptor"),
		},
	})
}

async fn connect(transport: &LocalTransportAuthority, version: ProtocolVersion) -> Client {
	let stream = transport.connect().await.expect("connect admitted local stream");
	let (mut client, _) =
		tokio_tungstenite::client_async_with_config(LOCAL_WEBSOCKET_URI, stream, None)
			.await
			.expect("connect real WebSocket client");

	send(
		&mut client,
		ClientMessage::Hello(ClientHello {
			version,
			artifact_cohort: Some(decodex_protocol::CURRENT_ARTIFACT_COHORT),
			expected_server_id: None,
			resume: None,
		}),
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
			artifact_cohort: Some(decodex_protocol::CURRENT_ARTIFACT_COHORT),
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
async fn exact_current_and_pre_payload_version_refusals_use_real_websockets() {
	let (_temp, transport) = local_transport();
	let application = FixtureApplication::default();
	let mut bound = server("version-server", application.clone(), ServerConfig::default())
		.bind(transport.clone())
		.await
		.expect("test operation must succeed");

	let mut client = connect(&transport, ProtocolVersion { major: 1, minor: 5 }).await;
	let ServerMessage::Refusal(refusal) = receive(&mut client).await else {
		panic!("expected version refusal");
	};

	assert!(matches!(
		refusal.refusal,
		Refusal::UnsupportedVersion(VersionRefusal::MajorMismatch { .. })
	));
	assert_eq!((application.queries(), application.executions()), (0, 0));

	drop(client);

	let mut client = connect(&transport, ProtocolVersion { major: 2, minor: 5 }).await;
	let ServerMessage::Refusal(refusal) = receive(&mut client).await else {
		panic!("expected minor-version refusal");
	};

	assert!(matches!(
		refusal.refusal,
		Refusal::UnsupportedVersion(VersionRefusal::UnsupportedMinor { .. })
	));
	assert_eq!((application.queries(), application.executions()), (0, 0));

	drop(client);

	let mut client = connect(&transport, CURRENT_VERSION).await;
	let ServerMessage::Welcome(welcome) = receive(&mut client).await else {
		panic!("expected welcome");
	};
	assert_eq!(welcome.version, CURRENT_VERSION);
	assert_eq!(welcome.supported.minimum_minor, 12);
	assert_eq!(welcome.supported.maximum_minor, 12);
	assert!(welcome.instance_id.is_some());
	assert!(matches!(receive(&mut client).await, ServerMessage::Snapshot(_)));
	execute_and_receive_event(&mut client, CURRENT_VERSION, 1).await;
	client.close(None).await.expect("test operation must succeed");

	bound.shutdown().await.expect("test operation must succeed");
}

#[tokio::test]
async fn post_negotiation_payload_envelopes_require_exact_current_version() {
	let (_temp, transport) = local_transport();
	let application = FixtureApplication::default();
	let mut bound = server("exact-feature-gate", application.clone(), ServerConfig::default())
		.bind(transport.clone())
		.await
		.expect("bind exact-current feature-gate server");
	let mut client = connect(&transport, CURRENT_VERSION).await;

	receive_initial(&mut client).await;
	send(
		&mut client,
		ClientMessage::Query(QueryEnvelope {
			version: ProtocolVersion { major: 2, minor: 5 },
			query_id: QueryId::new("future-query").expect("bounded query ID"),
			payload: QueryPayload::GetDoctorStatus,
		}),
	)
	.await;
	assert!(matches!(receive(&mut client).await, ServerMessage::Refusal(_)));
	assert_eq!(application.queries(), 0);

	send(&mut client, command(ProtocolVersion { major: 1, minor: 5 }, 1, "legacy-command")).await;
	assert!(matches!(receive(&mut client).await, ServerMessage::Refusal(_)));
	assert_eq!(application.executions(), 0);

	send(&mut client, doctor_query_for(CURRENT_VERSION, 1)).await;
	let ServerMessage::QueryResult(result) = receive(&mut client).await else {
		panic!("exact-current doctor query must be supported");
	};
	assert_eq!(result.version, CURRENT_VERSION);
	assert!(matches!(result.payload, QueryResultPayload::DoctorStatus(_)));
	assert_eq!(application.queries(), 1);
	execute_and_receive_event(&mut client, CURRENT_VERSION, 1).await;
	assert_eq!(application.executions(), 1);

	drop(client);
	bound.shutdown().await.expect("shutdown exact-current feature-gate server");
}

#[tokio::test]
async fn account_login_exchange_does_not_advance_or_enter_retained_state() {
	let (_temp, transport) = local_transport();
	let application = FixtureApplication::default();
	let mut bound = server("ephemeral-account-login", application.clone(), ServerConfig::default())
		.bind(transport.clone())
		.await
		.expect("bind ephemeral account-login server");
	let mut first = connect(&transport, CURRENT_VERSION).await;

	assert!(matches!(receive(&mut first).await, ServerMessage::Welcome(_)));
	let ServerMessage::Snapshot(before) = receive(&mut first).await else {
		panic!("expected initial snapshot");
	};
	send(&mut first, account_login_start()).await;
	let ServerMessage::AccountLogin(response) = receive(&mut first).await else {
		panic!("expected dedicated account-login response");
	};
	assert_eq!(response.version, CURRENT_VERSION);
	assert_eq!(response.status.state, AccountLoginState::OpeningBrowser);
	assert_eq!(application.login_requests(), 1);
	assert_eq!((application.queries(), application.executions()), (0, 0));
	drop(first);

	let mut second = connect(&transport, CURRENT_VERSION).await;
	assert!(matches!(receive(&mut second).await, ServerMessage::Welcome(_)));
	let ServerMessage::Snapshot(after) = receive(&mut second).await else {
		panic!("expected fresh snapshot");
	};
	assert_eq!(after.cursor, before.cursor);
	assert_eq!(after.items, before.items);
	assert!(
		!serde_json::to_string(&after)
			.expect("serialize credential-negative snapshot")
			.contains(response.status.session_id.as_str())
	);

	drop(second);
	bound.shutdown().await.expect("shutdown ephemeral account-login server");
}

#[tokio::test]
async fn v2_reset_card_events_reach_each_exact_current_subscriber() {
	let (_temp, transport) = local_transport();
	let application = FixtureApplication::with_revision(1);
	let mut bound =
		server("reset-event-feature-gate", application.clone(), ServerConfig::default())
			.bind(transport.clone())
			.await
			.expect("bind reset-card event feature-gate server");
	let mut observer = connect(&transport, CURRENT_VERSION).await;
	let mut current = connect(&transport, CURRENT_VERSION).await;

	receive_initial(&mut observer).await;
	receive_initial(&mut current).await;
	send(&mut current, reset_card_command(CURRENT_VERSION, 1, "current-reset-key")).await;

	assert!(matches!(receive(&mut current).await, ServerMessage::CommandReceipt(_)));
	assert!(matches!(receive(&mut current).await, ServerMessage::CommandResult(_)));
	let ServerMessage::Event(event) = receive(&mut current).await else {
		panic!("current client must receive the reset-card event");
	};

	assert_eq!(event.version, CURRENT_VERSION);
	assert!(matches!(event.payload, EventPayload::ResetCardOperationAccepted { .. }));
	let ServerMessage::Event(observer_event) = receive(&mut observer).await else {
		panic!("the second current subscriber must receive the reset-card event");
	};
	assert_eq!(observer_event.version, CURRENT_VERSION);
	assert!(matches!(observer_event.payload, EventPayload::ResetCardOperationAccepted { .. }));
	send(&mut observer, doctor_query_for(CURRENT_VERSION, 2)).await;
	let ServerMessage::QueryResult(result) = receive(&mut observer).await else {
		panic!("the second current subscriber must remain usable after the event");
	};
	assert_eq!(result.version, CURRENT_VERSION);
	assert!(matches!(result.payload, QueryResultPayload::DoctorStatus(_)));
	assert_eq!(application.executions(), 1);

	drop(current);
	bound.shutdown().await.expect("shutdown reset-card event feature-gate server");
}

#[tokio::test]
async fn duplicate_command_returns_the_original_receipt_and_mutates_once() {
	let (_temp, transport) = local_transport();
	let application = FixtureApplication::default();
	let mut bound = server("idempotency-server", application.clone(), ServerConfig::default())
		.bind(transport.clone())
		.await
		.expect("test operation must succeed");
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

	let mut client =
		reconnect(&transport, CURRENT_VERSION, ResumeCursor { server_id, instance_id, cursor })
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

	bound.shutdown().await.expect("test operation must succeed");
}

#[tokio::test]
async fn expected_revision_mismatch_is_rejected_without_publication() {
	let (_temp, transport) = local_transport();
	let application = FixtureApplication::default();
	let mut bound = server("revision-server", application.clone(), ServerConfig::default())
		.bind(transport.clone())
		.await
		.expect("test operation must succeed");
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

	bound.shutdown().await.expect("test operation must succeed");
}

#[tokio::test]
async fn acceptance_unknown_is_not_cached_and_same_key_can_recover() {
	let (_temp, transport) = local_transport();
	let application = FixtureApplication::with_acceptance_unknown_once();
	let mut bound =
		server("acceptance-unknown-server", application.clone(), ServerConfig::default())
			.bind(transport.clone())
			.await
			.expect("test operation must succeed");
	let mut client = connect(&transport, CURRENT_VERSION).await;

	receive_initial(&mut client).await;
	send(&mut client, command(CURRENT_VERSION, 1, "recovery-key")).await;

	let ServerMessage::CommandReceipt(first_receipt) = receive(&mut client).await else {
		panic!("expected first receipt");
	};
	let ServerMessage::CommandResult(first_result) = receive(&mut client).await else {
		panic!("expected first result");
	};

	assert_eq!(first_receipt.disposition, ReceiptDisposition::Executed);
	assert_eq!(first_result.outcome, decodex_protocol::CommandOutcome::AcceptanceUnknown);
	assert_eq!(first_result.error, Some(CommandError::AcceptanceUnknown));
	assert!(time::timeout(Duration::from_millis(100), client.next()).await.is_err());

	send(&mut client, command(CURRENT_VERSION, 2, "recovery-key")).await;

	let ServerMessage::CommandReceipt(second_receipt) = receive(&mut client).await else {
		panic!("expected recovery receipt");
	};

	assert_eq!(second_receipt.disposition, ReceiptDisposition::Executed);
	assert!(matches!(
		receive(&mut client).await,
		ServerMessage::CommandResult(decodex_protocol::CommandResultEnvelope {
			outcome: decodex_protocol::CommandOutcome::Succeeded,
			..
		})
	));
	assert!(matches!(receive(&mut client).await, ServerMessage::Event(_)));
	assert_eq!(application.attempts(), 2);
	assert_eq!(application.executions(), 1);

	drop(client);
	bound.shutdown().await.expect("test operation must succeed");
}

#[tokio::test]
async fn reconnect_resumes_ordered_deltas_and_falls_back_to_a_snapshot() {
	let (_temp, transport) = local_transport();
	let config = ServerConfig { replay_capacity: 2, ..ServerConfig::default() };
	let mut bound = server("resume-server", FixtureApplication::default(), config)
		.bind(transport.clone())
		.await
		.expect("test operation must succeed");
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
		ResumeCursor {
			server_id: server_id.clone(),
			instance_id: instance_id.clone(),
			cursor: Cursor(0),
		},
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

	bound.shutdown().await.expect("test operation must succeed");
}

#[tokio::test]
async fn bounded_outbound_queue_disconnects_a_slow_real_client() {
	for run in 0..OVERFLOW_STRESS_RUNS {
		assert_initiator_overflow(run).await;
	}
}

async fn assert_initiator_overflow(run: u64) {
	let (_temp, transport) = local_transport();
	let config = ServerConfig { outbound_queue_capacity: 1, ..ServerConfig::default() };
	let application = FixtureApplication::default();
	let server_id = format!("backpressure-server-{run}");
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

	let mut resumed =
		reconnect(&transport, CURRENT_VERSION, ResumeCursor { server_id, instance_id, cursor })
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
	let (_temp, transport) = local_transport();
	let config = ServerConfig { outbound_queue_capacity: 2, ..ServerConfig::default() };
	let application = FixtureApplication::default();
	let server_id = format!("event-overflow-{run}");
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

	let mut resumed =
		reconnect(&transport, CURRENT_VERSION, ResumeCursor { server_id, instance_id, cursor })
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
	let (_temp, transport) = local_transport();
	let application = FixtureApplication::with_delay(Duration::from_millis(200));
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
	let (_temp, transport) = local_transport();
	let config = ServerConfig { receipt_capacity: 1, ..ServerConfig::default() };
	let application = FixtureApplication::default();
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
	let (_temp, transport) = local_transport();
	let config = ServerConfig { receipt_capacity: 1, ..ServerConfig::default() };
	let application = FixtureApplication::default();
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
		first_report.check(DoctorComponent::ProductStore).expect("database check").status,
		DoctorStatus::Ready
	);
	assert_eq!(
		second_report.check(DoctorComponent::ProductStore).expect("database check").status,
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
	let (_temp, transport) = local_transport();
	let application = FixtureApplication::default();
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
async fn receipt_capacity_has_one_exact_current_namespace() {
	let (_temp, transport) = local_transport();
	let config = ServerConfig { receipt_capacity: 1, ..ServerConfig::default() };
	let application = FixtureApplication::default();
	let mut bound = server("exact-current-receipt-capacity", application.clone(), config)
		.bind(transport.clone())
		.await
		.expect("bind exact-current receipt-capacity server");
	let mut first = connect(&transport, CURRENT_VERSION).await;

	receive_initial(&mut first).await;
	execute_and_receive_event(&mut first, CURRENT_VERSION, 1).await;
	send(&mut first, command(CURRENT_VERSION, 2, "key-1")).await;

	let ServerMessage::CommandReceipt(duplicate) = receive(&mut first).await else {
		panic!("expected same-version duplicate at capacity");
	};

	assert_eq!(duplicate.disposition, ReceiptDisposition::Duplicate);
	assert!(matches!(receive(&mut first).await, ServerMessage::CommandResult(_)));

	let mut conflict_message = command(CURRENT_VERSION, 3, "key-1");
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

	let mut second = connect(&transport, CURRENT_VERSION).await;

	receive_initial(&mut second).await;
	send(&mut second, command(CURRENT_VERSION, 4, "key-1")).await;
	let ServerMessage::CommandReceipt(replayed) = receive(&mut second).await else {
		panic!("expected exact-current replay");
	};
	assert_eq!(replayed.disposition, ReceiptDisposition::Duplicate);
	assert!(matches!(receive(&mut second).await, ServerMessage::CommandResult(_)));
	send(&mut second, command(CURRENT_VERSION, 5, "new-key-at-capacity")).await;

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
	assert_eq!(application.executions(), 1);

	drop(second);

	bound.shutdown().await.expect("shutdown exact-current receipt-capacity server");
}

#[tokio::test]
async fn oversized_outbound_snapshot_is_closed_before_transmission() {
	let (_temp, transport) = local_transport();
	let config = ServerConfig { maximum_message_bytes: 512, ..ServerConfig::default() };
	let application = FixtureApplication::with_status(
		WireText::new("x".repeat(1_024)).expect("fixture remains below scalar limit"),
	);
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
	let (_temp, transport) = local_transport();
	let application = FixtureApplication::default();
	let mut bound = server("bounded-identifiers", application.clone(), ServerConfig::default())
		.bind(transport.clone())
		.await
		.expect("bind bounded-identifiers server");
	let mut client = connect(&transport, CURRENT_VERSION).await;

	receive_initial(&mut client).await;

	let raw = serde_json::json!({
		"type": "command",
		"body": {
			"version": CURRENT_VERSION,
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
async fn malformed_and_abandoned_pre_registration_sessions_are_session_local() {
	let (_temp, transport) = local_transport();
	let mut bound = server(
		"pre-registration-peer-failures",
		FixtureApplication::default(),
		ServerConfig::default(),
	)
	.bind(transport.clone())
	.await
	.expect("bind pre-registration peer-failure server");

	let stream = transport.connect().await.expect("connect abandoned local stream");
	let (abandoned, _) =
		tokio_tungstenite::client_async_with_config(LOCAL_WEBSOCKET_URI, stream, None)
			.await
			.expect("connect abandoned real WebSocket client");
	drop(abandoned);

	let stream = transport.connect().await.expect("connect malformed local stream");
	let (mut malformed, _) =
		tokio_tungstenite::client_async_with_config(LOCAL_WEBSOCKET_URI, stream, None)
			.await
			.expect("connect malformed real WebSocket client");
	malformed
		.send(Message::Text("{".into()))
		.await
		.expect("send malformed first WebSocket message");
	let ServerMessage::Refusal(refusal) = receive(&mut malformed).await else {
		panic!("expected malformed first-message refusal");
	};
	assert!(matches!(refusal.refusal, Refusal::ProtocolViolation { .. }));
	drop(malformed);

	let mut healthy = connect(&transport, CURRENT_VERSION).await;
	receive_initial(&mut healthy).await;
	drop(healthy);

	let receipt = bound.shutdown().await.expect("requested shutdown remains exact");
	assert_eq!(
		receipt,
		TerminationReceipt {
			primary: TerminationPrimary::RequestedShutdown,
			spawned_sessions: 3,
			spawned_services: 0,
			actor_commands_admitted: 0,
			actor_commands_settled: 0,
			actor_command_deadline: ActorCommandDeadlineClass::NoActiveCommand,
			harvested_tasks: 3,
			expected_tasks: 3,
			panicked_tasks: 0,
			failed_tasks: 0,
			forced_cancelled_tasks: 0,
			owner_integrity_failures: 0,
			lowest_panicked: None,
			lowest_failed: None,
			lowest_forced: None,
			endpoint_refusal: None,
			cleanup_refusal: None,
		},
	);
}

#[tokio::test]
async fn daemon_service_settlement_holds_namespace_authority_until_zero_survivor() {
	let (_temp, transport) = local_transport();
	let service = Arc::new(FixtureDaemonService::default());
	let application = FixtureApplication::with_daemon_service(Arc::clone(&service));
	let mut bound = server("service-settlement", application, ServerConfig::default())
		.bind(transport.clone())
		.await
		.expect("bind service-settlement server");

	time::timeout(Duration::from_secs(1), service.started.notified())
		.await
		.expect("daemon service must start under the server lifecycle");
	let shutdown = tokio::spawn(async move { bound.shutdown().await });

	time::timeout(Duration::from_secs(1), service.stop_observed.notified())
		.await
		.expect("daemon service must observe stopping");
	let contender = server(
		"service-settlement-contender",
		FixtureApplication::default(),
		ServerConfig::default(),
	)
	.bind(transport.clone())
	.await;

	assert!(matches!(
		contender,
		Err(decodex_runtime::ServerError::LocalTransport(LocalTransportRefusal::EndpointInUse))
	));

	service.release.notify_one();
	time::timeout(Duration::from_secs(2), shutdown)
		.await
		.expect("server must finish after registered service work settles")
		.expect("server lifecycle task must not panic")
		.expect("service-settlement shutdown must succeed");

	let mut restarted = server(
		"service-settlement-restarted",
		FixtureApplication::default(),
		ServerConfig::default(),
	)
	.bind(transport)
	.await
	.expect("namespace authority must release after service settlement");

	restarted.shutdown().await.expect("shutdown restarted service-settlement server");
}

#[tokio::test]
async fn missing_canonical_publication_stops_established_service_and_allows_rebind() {
	let (temp, transport) = local_transport();
	let socket_path = temp.path().join(".decodex/server/decodex.sock");
	let mut bound = server(
		"missing-listener-publication",
		FixtureApplication::default(),
		ServerConfig::default(),
	)
	.bind(transport.clone())
	.await
	.expect("bind listener-loss fixture server");
	let mut established = connect(&transport, CURRENT_VERSION).await;

	receive_initial(&mut established).await;
	fs::remove_file(&socket_path).expect("remove canonical publication");

	let error = time::timeout(Duration::from_secs(2), bound.wait())
		.await
		.expect("listener loss must stop the service")
		.expect_err("listener loss must be abnormal");
	let ServerError::Terminated(receipt) = error else {
		panic!("expected deterministic terminated receipt, got {error:?}");
	};
	assert_eq!(receipt.endpoint_refusal, Some(LocalTransportRefusal::EndpointReplaced));
	assert_eq!(receipt.cleanup_refusal, Some(LocalTransportRefusal::EndpointReplaced));
	let old_stream = time::timeout(Duration::from_secs(1), established.next())
		.await
		.expect("established stream must stop with the lost publication");
	assert!(matches!(old_stream, None | Some(Ok(Message::Close(_))) | Some(Err(_))));

	let mut restarted = server(
		"missing-listener-publication-restarted",
		FixtureApplication::default(),
		ServerConfig::default(),
	)
	.bind(transport.clone())
	.await
	.expect("app-owned recovery must republish after listener loss");
	let mut reconnected = connect(&transport, CURRENT_VERSION).await;

	receive_initial(&mut reconnected).await;
	drop(reconnected);
	drop(established);
	restarted.shutdown().await.expect("shutdown republished fixture server");
}

#[tokio::test]
async fn replacement_publication_stops_service_without_unlinking_replacement() {
	let (temp, transport) = local_transport();
	let socket_path = temp.path().join(".decodex/server/decodex.sock");
	let retained_path = socket_path.with_file_name("retained.sock");
	let mut bound = server(
		"replaced-listener-publication",
		FixtureApplication::default(),
		ServerConfig::default(),
	)
	.bind(transport.clone())
	.await
	.expect("bind listener-replacement fixture server");
	let mut established = connect(&transport, CURRENT_VERSION).await;

	receive_initial(&mut established).await;
	fs::rename(&socket_path, &retained_path).expect("move owned publication aside");
	let replacement =
		StandardUnixListener::bind(&socket_path).expect("publish unowned replacement socket");
	fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600))
		.expect("scope replacement socket");

	let error = time::timeout(Duration::from_secs(2), bound.wait())
		.await
		.expect("listener replacement must stop the service")
		.expect_err("listener replacement must be abnormal");
	let ServerError::Terminated(receipt) = error else {
		panic!("expected deterministic terminated receipt, got {error:?}");
	};
	assert_eq!(receipt.endpoint_refusal, Some(LocalTransportRefusal::EndpointReplaced));
	assert_eq!(receipt.cleanup_refusal, Some(LocalTransportRefusal::EndpointReplaced));
	assert!(socket_path.exists(), "cleanup must preserve an unowned replacement");
	assert!(matches!(
		server("replacement-contender", FixtureApplication::default(), ServerConfig::default(),)
			.bind(transport)
			.await,
		Err(ServerError::LocalTransport(LocalTransportRefusal::EndpointInUse))
	));

	drop(established);
	drop(replacement);
	fs::remove_file(&socket_path).expect("remove replacement fixture socket");
	fs::remove_file(&retained_path).expect("remove retained fixture socket");
}

#[tokio::test]
async fn restart_with_stable_server_identity_rejects_equal_and_overlapping_old_epoch_cursors() {
	let (_temp, transport) = local_transport();
	let application = FixtureApplication::default();
	let mut first = server("stable-restart", application.clone(), ServerConfig::default())
		.bind(transport.clone())
		.await
		.expect("test operation must succeed");
	let mut client = connect(&transport, CURRENT_VERSION).await;
	let (server_id, old_instance_id, initial_cursor, _) = receive_initial(&mut client).await;
	let overlapping_cursor = execute_and_receive_event(&mut client, CURRENT_VERSION, 1).await;

	drop(client);

	first.shutdown().await.expect("test operation must succeed");

	let mut restarted = server("stable-restart", application, ServerConfig::default())
		.bind(transport.clone())
		.await
		.expect("test operation must succeed");
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

	restarted.shutdown().await.expect("test operation must succeed");
}

#[tokio::test]
async fn cross_uid_authority_is_refused_before_opening_a_socket() {
	let temp = TempDir::new().expect("create cross-UID test root");
	let root = DecodexRoot::new(
		temp.path().canonicalize().expect("canonical cross-UID temp").join(".decodex"),
	)
	.expect("create typed test root");

	root.paths().ensure_layout().expect("create cross-UID test layout");

	let effective_uid = unsafe { libc::geteuid() };
	let error = LocalTransportAuthority::new(
		root.paths(),
		LocalTrustPolicy::SameUid,
		Some(effective_uid ^ 1),
	)
	.expect_err("mismatched service UID must fail before socket creation");

	assert_eq!(error, LocalTransportRefusal::EffectiveUidMismatch);
}
