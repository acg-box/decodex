//! SQLite bootstrap and authoritative doctor protocol fixtures.
#![allow(unused_crate_dependencies)]

#[cfg(unix)] use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
use std::{fs, fs::OpenOptions, io::Write as _};

use futures_util::{SinkExt as _, StreamExt as _};
use tempfile::TempDir;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_tungstenite::{WebSocketStream, tungstenite::Message};

use decodex_core::{Availability, DecodexRoot, LocalTrustPolicy};
use decodex_protocol::{
	AppServerCapability, CURRENT_VERSION, ClientHello, ClientMessage, ConversationHistoryResult,
	DoctorComponent, DoctorIssue, DoctorStatus, EntityId, HistoryQueryError,
	LocalTransportAuthority, LocalTransportRefusal, LocalTransportStream, ProtocolVersion,
	QueryEnvelope, QueryId, QueryPayload, QueryResultPayload, Refusal, ServerId, ServerMessage,
	VersionRefusal,
};
use decodex_runtime::{ServerConfig, ServiceBootstrap, ServiceComposition};

// Handshake metadata only. The stream is already admitted by the local authority.
const LOCAL_WEBSOCKET_URI: &str = "ws://localhost/v1/ws";

fn root(temp: &TempDir) -> DecodexRoot {
	DecodexRoot::new(temp.path().canonicalize().expect("canonical fixture temp").join(".decodex"))
		.expect("fixture root is safe")
}

fn local_config() -> String {
	format!(
		r#"version = 1
active_profile = "local"

[profiles.local]
kind = "local"
policy = "same_uid"
service_owner_uid = {}

[cache]
max_entries = 16
max_bytes = 65536
max_entry_bytes = 4096
"#,
		// SAFETY: `geteuid` has no arguments or failure return.
		unsafe { libc::geteuid() },
	)
}

fn write_config(root: &DecodexRoot, body: &str) {
	let paths = root.paths();

	paths.ensure_layout().expect("create private fixture layout");

	let mut options = OpenOptions::new();
	options.create(true).truncate(true).write(true);
	#[cfg(unix)]
	options.mode(0o600);

	let mut file = options.open(paths.config_file()).expect("open fixture config");
	file.write_all(body.as_bytes()).expect("write fixture config");
	file.sync_all().expect("sync fixture config");

	#[cfg(unix)]
	assert_eq!(
		file.metadata().expect("fixture config metadata").permissions().mode() & 0o777,
		0o600
	);
}

fn local_transport(root: &DecodexRoot) -> LocalTransportAuthority {
	let paths = root.paths();
	paths.ensure_layout().expect("create owner-only local transport layout");

	// SAFETY: `geteuid` has no arguments or failure return.
	let service_owner_uid = unsafe { libc::geteuid() };

	LocalTransportAuthority::new(paths, LocalTrustPolicy::SameUid, Some(service_owner_uid))
		.expect("same-UID local transport authority")
}

async fn connect_local(
	transport: &LocalTransportAuthority,
) -> WebSocketStream<LocalTransportStream> {
	let stream = transport.connect().await.expect("connect admitted local stream");
	let (socket, _) =
		tokio_tungstenite::client_async_with_config(LOCAL_WEBSOCKET_URI, stream, None)
			.await
			.expect("complete local WebSocket handshake");

	socket
}

fn status(bootstrap: &ServiceBootstrap, component: DoctorComponent) -> DoctorStatus {
	bootstrap.doctor().check(component).expect("doctor component is present").status
}

fn doctor_query(version: ProtocolVersion, query_id: &str) -> QueryEnvelope {
	QueryEnvelope {
		version,
		query_id: QueryId::new(query_id).expect("bounded query ID"),
		payload: QueryPayload::GetDoctorStatus,
	}
}

#[tokio::test]
async fn missing_malformed_and_redacted_bootstrap_are_typed() {
	let missing_temp = TempDir::new().expect("missing-config temp");
	let missing = ServiceComposition::bootstrap(root(&missing_temp)).await;

	assert_eq!(
		status(&missing, DoctorComponent::Configuration),
		DoctorStatus::Unavailable(DoctorIssue::ConfigurationMissing)
	);
	assert_eq!(
		status(&missing, DoctorComponent::ProductStore),
		DoctorStatus::Unavailable(DoctorIssue::DatabaseNotConfigured)
	);
	assert_eq!(
		missing.product_state_availability(),
		Availability::Unavailable { reason: "local product database configuration is unavailable" }
	);

	let malformed_temp = TempDir::new().expect("malformed-config temp");
	let malformed_root = root(&malformed_temp);
	let secret = "fixture-password-must-never-leak";
	write_config(&malformed_root, &format!("version = 1\npassword = \"{secret}\"\n"));

	let malformed = ServiceComposition::bootstrap(malformed_root).await;
	let encoded = serde_json::to_string(malformed.doctor()).expect("encode redacted doctor");

	assert_eq!(
		status(&malformed, DoctorComponent::Configuration),
		DoctorStatus::Unavailable(DoctorIssue::ConfigurationMalformed)
	);
	assert_eq!(
		status(&malformed, DoctorComponent::ProductStore),
		DoctorStatus::Unavailable(DoctorIssue::DatabaseMalformedConfig)
	);
	assert!(!encoded.contains(secret));
	assert!(!encoded.contains(malformed_temp.path().to_string_lossy().as_ref()));
}

#[tokio::test]
async fn singleton_authority_precedes_identity_and_database_bootstrap() {
	let temp = TempDir::new().expect("singleton-order temp");
	let decodex_root = root(&temp);
	write_config(&decodex_root, &local_config());

	let paths = decodex_root.paths();
	let authority = local_transport(&decodex_root);
	let listener = authority.bind().await.expect("hold singleton authority");

	assert!(!paths.server_identity_file().exists());

	let blocked = ServiceComposition::bootstrap(decodex_root).await;
	assert!(
		!paths.server_identity_file().exists(),
		"a blocked daemon must not create identity or SQLite before singleton authority",
	);
	assert!(!paths.product_database_file().exists());
	assert!(matches!(
		blocked.bind(ServerConfig::default()).await,
		Err(decodex_runtime::ServerError::LocalTransport(LocalTransportRefusal::EndpointInUse))
	));

	listener.cleanup().expect("release singleton fixture authority");
}

#[cfg(unix)]
#[tokio::test]
async fn symlinked_database_is_rejected_before_sqlite_open() {
	let temp = TempDir::new().expect("symlinked-database temp");
	let decodex_root = root(&temp);
	write_config(&decodex_root, &local_config());
	let paths = decodex_root.paths();
	let external = temp.path().join("external.sqlite3");

	fs::write(&external, b"not a product database").expect("write external database fixture");
	std::os::unix::fs::symlink(&external, paths.product_database_file())
		.expect("symlink database fixture");

	let bootstrap = ServiceComposition::bootstrap(decodex_root).await;

	assert_eq!(status(&bootstrap, DoctorComponent::Configuration), DoctorStatus::Ready);
	assert_eq!(
		status(&bootstrap, DoctorComponent::ProductStore),
		DoctorStatus::Unavailable(DoctorIssue::UnsafeHostPath)
	);
	assert!(matches!(bootstrap.product_state_availability(), Availability::Unavailable { .. }));
}

#[cfg(unix)]
#[tokio::test]
async fn symlinked_owned_config_is_unsafe_not_malformed() {
	let temp = TempDir::new().expect("symlinked-config temp");
	let decodex_root = root(&temp);
	let paths = decodex_root.paths();
	let external = temp.path().join("external-config.toml");

	paths.ensure_layout().expect("create fixture layout");
	fs::write(&external, "version = 1\n").expect("write external config");
	std::os::unix::fs::symlink(&external, paths.config_file()).expect("symlink config fixture");

	let bootstrap = ServiceComposition::bootstrap(decodex_root).await;

	assert_eq!(
		status(&bootstrap, DoctorComponent::Configuration),
		DoctorStatus::Unavailable(DoctorIssue::UnsafeHostPath)
	);
	assert_eq!(
		status(&bootstrap, DoctorComponent::ProductStore),
		DoctorStatus::Unavailable(DoctorIssue::UnsafeHostPath)
	);
}

#[tokio::test]
async fn fresh_sqlite_bootstrap_is_ready_and_deferred_surfaces_are_explicit() {
	let temp = TempDir::new().expect("fresh SQLite temp");
	let decodex_root = root(&temp);
	write_config(&decodex_root, &local_config());

	let bootstrap = ServiceComposition::bootstrap(decodex_root.clone()).await;

	assert_eq!(status(&bootstrap, DoctorComponent::Configuration), DoctorStatus::Ready);
	assert_eq!(status(&bootstrap, DoctorComponent::ProductStore), DoctorStatus::Ready);
	assert_eq!(status(&bootstrap, DoctorComponent::Protocol), DoctorStatus::Ready);
	assert_eq!(status(&bootstrap, DoctorComponent::ProtocolVersion), DoctorStatus::Ready);
	assert_eq!(status(&bootstrap, DoctorComponent::ServerIdentity), DoctorStatus::Ready);
	assert_eq!(
		status(&bootstrap, DoctorComponent::ManagedRepository),
		DoctorStatus::Unavailable(DoctorIssue::Disabled)
	);
	assert_eq!(
		status(&bootstrap, DoctorComponent::BlobIntegrity),
		DoctorStatus::Unknown(DoctorIssue::NotProbed)
	);
	assert_eq!(bootstrap.product_state_availability(), Availability::Available);
	assert!(decodex_root.paths().product_database_file().is_file());

	for capability in AppServerCapability::ALL {
		assert_eq!(
			status(&bootstrap, DoctorComponent::AppServerCapability(capability)),
			DoctorStatus::Unknown(DoctorIssue::NotProbed)
		);
	}
}

#[tokio::test]
async fn doctor_crosses_the_daemon_protocol_and_wrong_server_is_refused() {
	let temp = TempDir::new().expect("protocol temp");
	let decodex_root = root(&temp);
	write_config(&decodex_root, &local_config());

	let transport = local_transport(&decodex_root);
	let bootstrap = ServiceComposition::bootstrap(decodex_root).await;
	let server_id = bootstrap.server_id().clone();
	let mut bound = bootstrap.bind(ServerConfig::default()).await.expect("bind daemon fixture");
	let mut wrong = connect_local(&transport).await;

	send(
		&mut wrong,
		ClientMessage::Hello(ClientHello {
			version: CURRENT_VERSION,
			artifact_cohort: Some(decodex_protocol::CURRENT_ARTIFACT_COHORT),
			expected_server_id: Some(ServerId::new("wrong-server").expect("bounded wrong ID")),
			resume: None,
		}),
	)
	.await;

	let ServerMessage::Refusal(refusal) = receive(&mut wrong).await else {
		panic!("expected wrong-server refusal");
	};
	assert!(matches!(
		refusal.refusal,
		Refusal::ServerIdentityMismatch{ expected, actual }
			if expected == ServerId::new("wrong-server").expect("bounded wrong ID")
				&& actual == server_id
	));

	let mut client = connect_local(&transport).await;
	send(
		&mut client,
		ClientMessage::Hello(ClientHello {
			version: CURRENT_VERSION,
			artifact_cohort: Some(decodex_protocol::CURRENT_ARTIFACT_COHORT),
			expected_server_id: Some(server_id.clone()),
			resume: None,
		}),
	)
	.await;

	assert!(matches!(receive(&mut client).await, ServerMessage::Welcome(_)));
	assert!(matches!(receive(&mut client).await, ServerMessage::Snapshot(_)));

	send(&mut client, ClientMessage::Query(doctor_query(CURRENT_VERSION, "doctor-query"))).await;
	let ServerMessage::QueryResult(result) = receive(&mut client).await else {
		panic!("expected doctor result");
	};
	let QueryResultPayload::DoctorStatus(report) = result.payload else {
		panic!("expected doctor result");
	};
	assert_eq!(report.server_id(), &server_id);
	assert_eq!(report.version(), CURRENT_VERSION);

	send(
		&mut client,
		ClientMessage::Query(QueryEnvelope {
			version: CURRENT_VERSION,
			query_id: QueryId::new("history-missing").expect("bounded query ID"),
			payload: QueryPayload::GetConversationHistory {
				conversation_id: EntityId::new("40000000-0000-4000-8000-000000000001")
					.expect("bounded Conversation ID"),
				after: None,
				page_size: 1,
			},
		}),
	)
	.await;
	assert!(matches!(
		receive(&mut client).await,
		ServerMessage::QueryResult(result)
			if matches!(
				result.payload,
				QueryResultPayload::ConversationHistory(
					ConversationHistoryResult::Unavailable {
						error: HistoryQueryError::InvalidRequest,
					}
				)
			)
	));

	assert_exact_current_doctor_queries(&transport, &server_id).await;
	drop((wrong, client));
	bound.shutdown().await.expect("shutdown daemon fixture");
}

async fn assert_exact_current_doctor_queries(
	transport: &LocalTransportAuthority,
	server_id: &ServerId,
) {
	let mut legacy = connect_local(transport).await;
	send(
		&mut legacy,
		ClientMessage::Hello(ClientHello {
			version: ProtocolVersion { major: 1, minor: 5 },
			artifact_cohort: Some(decodex_protocol::CURRENT_ARTIFACT_COHORT),
			expected_server_id: Some(server_id.clone()),
			resume: None,
		}),
	)
	.await;
	let ServerMessage::Refusal(refusal) = receive(&mut legacy).await else {
		panic!("expected V1.5 major-version refusal");
	};
	assert!(matches!(
		refusal.refusal,
		Refusal::UnsupportedVersion(VersionRefusal::MajorMismatch { .. })
	));

	let mut future = connect_local(transport).await;
	send(
		&mut future,
		ClientMessage::Hello(ClientHello {
			version: ProtocolVersion { major: 2, minor: 6 },
			artifact_cohort: Some(decodex_protocol::CURRENT_ARTIFACT_COHORT),
			expected_server_id: Some(server_id.clone()),
			resume: None,
		}),
	)
	.await;
	let ServerMessage::Refusal(refusal) = receive(&mut future).await else {
		panic!("expected V2.5 minor-version refusal");
	};
	assert!(matches!(
		refusal.refusal,
		Refusal::UnsupportedVersion(VersionRefusal::UnsupportedMinor { .. })
	));

	let mut stale_cohort = connect_local(transport).await;
	send(
		&mut stale_cohort,
		ClientMessage::Hello(ClientHello {
			version: CURRENT_VERSION,
			artifact_cohort: None,
			expected_server_id: Some(server_id.clone()),
			resume: None,
		}),
	)
	.await;
	let ServerMessage::Refusal(refusal) = receive(&mut stale_cohort).await else {
		panic!("expected artifact-cohort refusal");
	};
	assert!(matches!(
		refusal.refusal,
		Refusal::ArtifactCohortMismatch {
			expected: decodex_protocol::CURRENT_ARTIFACT_COHORT,
			actual: None,
		}
	));

	let mut current = connect_local(transport).await;
	send(
		&mut current,
		ClientMessage::Hello(ClientHello {
			version: CURRENT_VERSION,
			artifact_cohort: Some(decodex_protocol::CURRENT_ARTIFACT_COHORT),
			expected_server_id: Some(server_id.clone()),
			resume: None,
		}),
	)
	.await;
	assert!(matches!(receive(&mut current).await, ServerMessage::Welcome(_)));
	assert!(matches!(receive(&mut current).await, ServerMessage::Snapshot(_)));

	send(
		&mut current,
		ClientMessage::Query(doctor_query(
			ProtocolVersion { major: 2, minor: 6 },
			"future-query-on-current-session",
		)),
	)
	.await;
	let ServerMessage::Refusal(result) = receive(&mut current).await else {
		panic!("expected mismatched future-minor doctor rejection");
	};
	assert!(matches!(result.refusal, Refusal::ProtocolViolation { .. }));
}

#[tokio::test]
async fn disconnected_fixture_is_deterministic() {
	let temp = TempDir::new().expect("disconnected temp");
	let decodex_root = root(&temp);
	let transport = local_transport(&decodex_root);

	assert!(matches!(transport.connect().await, Err(LocalTransportRefusal::EndpointUnavailable)));
}

async fn send<S>(client: &mut WebSocketStream<S>, message: ClientMessage)
where
	S: AsyncRead + AsyncWrite + Unpin,
{
	let encoded = serde_json::to_string(&message).expect("encode client message");
	client.send(Message::Text(encoded.into())).await.expect("send client message");
}

async fn receive<S>(client: &mut WebSocketStream<S>) -> ServerMessage
where
	S: AsyncRead + AsyncWrite + Unpin,
{
	let Message::Text(text) =
		client.next().await.expect("server message exists").expect("server message succeeds")
	else {
		panic!("expected text message");
	};

	serde_json::from_str(&text).expect("decode server message")
}
