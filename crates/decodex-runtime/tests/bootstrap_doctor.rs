//! XY-1307 typed fail-closed bootstrap and authoritative doctor protocol fixtures.
#![allow(unused_crate_dependencies)]

#[cfg(unix)]
use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _};
#[cfg(unix)] use std::os::unix::net::UnixListener;
use std::{
	env,
	fs::{self, OpenOptions, Permissions},
	io::Write as _,
	path::{Path, PathBuf},
	process::{self, Command},
	time::Duration,
};

use futures_util::{SinkExt as _, StreamExt as _};
use tempfile::TempDir;
use tokio::{
	io::{AsyncRead, AsyncWrite},
	time,
};
use tokio_tungstenite::{WebSocketStream, tungstenite::Message};

use decodex_core::{Availability, BlobHash, BlobStore, DecodexRoot, LocalTrustPolicy};
use decodex_protocol::{
	AppServerCapability, CURRENT_VERSION, ClientHello, ClientMessage, ConversationHistoryPage,
	ConversationHistoryResult, DoctorComponent, DoctorIssue, DoctorStatus, EntityId,
	HistoryCursorToken, HistoryItemKindDto, HistoryMetadataValue, HistoryPayloadDto,
	HistoryQueryError, LocalTransportAuthority, LocalTransportRefusal, LocalTransportStream,
	MAX_HISTORY_PAGE_SIZE, ProtocolVersion, QueryEnvelope, QueryId, QueryPayload,
	QueryResultPayload, Refusal, ServerId, ServerMessage, VersionRefusal,
};
use decodex_runtime::{ServerConfig, ServiceBootstrap, ServiceComposition};

// Handshake metadata only. The stream is already admitted by the local authority.
const LOCAL_WEBSOCKET_URI: &str = "ws://localhost/v1/ws";

#[cfg(unix)]
struct SocketDirectoryReplacement {
	configured: PathBuf,
	pinned: PathBuf,
	listener: Option<UnixListener>,
}
#[cfg(unix)]
impl SocketDirectoryReplacement {
	fn install(configured: PathBuf, port: u16) -> Self {
		let directory_name = configured
			.file_name()
			.and_then(|name| name.to_str())
			.expect("socket directory has a UTF-8 final component");
		let pinned =
			configured.with_file_name(format!("{directory_name}-xy1307-pinned-{}", process::id()));

		assert!(!pinned.exists(), "pinned fixture path must be unused");

		fs::rename(&configured, &pinned).expect("pin the live PostgreSQL socket directory");
		fs::create_dir(&configured).expect("create replacement socket directory");
		fs::set_permissions(&configured, Permissions::from_mode(0o700))
			.expect("secure replacement socket directory");

		let listener = UnixListener::bind(configured.join(format!(".s.PGSQL.{port}")))
			.expect("bind replacement endpoint");

		Self { configured, pinned, listener: Some(listener) }
	}
}

#[cfg(unix)]
impl Drop for SocketDirectoryReplacement {
	fn drop(&mut self) {
		drop(self.listener.take());

		fs::remove_dir_all(&self.configured).expect("remove replacement socket directory");
		fs::rename(&self.pinned, &self.configured)
			.expect("restore live PostgreSQL socket directory");
	}
}

fn root(temp: &TempDir) -> DecodexRoot {
	DecodexRoot::new(temp.path().canonicalize().expect("canonical fixture temp").join(".decodex"))
		.expect("fixture root is safe")
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

fn config(socket: &Path, database: &str, credential: Option<&str>) -> String {
	let runtime_credential = credential
		.map(|name| format!("credential_env_var = \"{name}_RUNTIME\"\n"))
		.unwrap_or_default();
	let fixture_user = env::var("USER").unwrap_or_else(|_| "postgres".into());

	format!(
		r#"version = 1
active_profile = "local"

[profiles.local]
kind = "local"
policy = "same_uid"
service_owner_uid = {}

[postgres]
socket_directory = "{}"
expected_peer_uid = {}
port = 5432
database = "{}"

[postgres.runtime]
user = "{}_runtime"
{}
[cache]
max_entries = 16
max_bytes = 65536
max_entry_bytes = 4096
"#,
		// SAFETY: `geteuid` has no arguments or failure return.
		unsafe { libc::geteuid() },
		socket.display(),
		env::current_dir()
			.expect("fixture current directory")
			.metadata()
			.expect("fixture owner metadata")
			.uid(),
		database,
		fixture_user,
		runtime_credential,
	)
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

fn history_query(id: &str, after: Option<&str>, page_size: u16) -> ClientMessage {
	history_query_for(id, "40000000-0000-4000-8000-000000000001", after, page_size)
}

fn history_query_for(
	id: &str,
	conversation_id: &str,
	after: Option<&str>,
	page_size: u16,
) -> ClientMessage {
	ClientMessage::Query(QueryEnvelope {
		version: CURRENT_VERSION,
		query_id: QueryId::new(id).expect("history query identity is bounded"),
		payload: QueryPayload::GetConversationHistory {
			conversation_id: EntityId::new(conversation_id)
				.expect("history Conversation identity is bounded"),
			after: after.map(|value| {
				HistoryCursorToken::new(value).expect("history cursor fixture is bounded")
			}),
			page_size,
		},
	})
}

fn expire_isolated_history_cursor(cursor: &str) {
	let cursor = cursor.strip_prefix("v1:").expect("fixture cursor is versioned");
	let statement = format!(
		"UPDATE decodex.history_cursors \
		 SET created_at=clock_timestamp()-interval '2 hours', \
		 expires_at=clock_timestamp()-interval '1 hour' \
		 WHERE cursor_id='{cursor}'::uuid"
	);

	run_isolated_sql(&statement);
}

fn exhaust_isolated_history_cursor_capacity() {
	run_isolated_sql(
		"DO $$ DECLARE parent uuid; BEGIN \
		 FOR position IN 1..511 LOOP \
		  parent := decodex.issue_history_cursor( \
		   '49000000-0000-4000-8000-000000000010',parent,1); \
		 END LOOP; END $$; \
		 DO $$ BEGIN IF (SELECT count(*) FROM decodex.history_cursors) <> 4096 \
		  OR (SELECT count(*) FROM decodex.history_cursors \
		  WHERE conversation_id='49000000-0000-4000-8000-000000000010') <> 511 \
		 THEN RAISE EXCEPTION 'cursor capacity fixture is incomplete'; END IF; END $$",
	);
}

fn run_isolated_sql(statement: &str) {
	let database_url = env::var("DECODEX_TEST_SCHEMA_OWNER_DATABASE_URL")
		.expect("isolated schema-owner URL is present");
	let output = Command::new("psql")
		.arg(database_url)
		.args(["-v", "ON_ERROR_STOP=1", "-Atqc", statement])
		.output()
		.expect("run isolated cursor expiry fixture");

	assert!(output.status.success(), "isolated cursor SQL fixture failed");
}

fn assert_history_projection(page: &ConversationHistoryPage) {
	let offloaded = page
		.items
		.iter()
		.find(|item| matches!(item.payload, HistoryPayloadDto::Blob(_)))
		.expect("offloaded history projection");

	assert_eq!(offloaded.media_type.as_str(), "text/plain");
	assert_eq!(
		offloaded.metadata.as_map().get("source"),
		Some(&HistoryMetadataValue::Text("synthetic".into())),
	);

	let json_item = page
		.items
		.iter()
		.find(|item| item.media_type.as_str() == "application/json")
		.expect("inline JSON history projection");

	assert!(matches!(json_item.payload, HistoryPayloadDto::Inline { .. }));
	assert_eq!(
		json_item.metadata.as_map().get("correlation"),
		Some(&HistoryMetadataValue::Text("synthetic-stream".into())),
	);
	assert_eq!(
		json_item.metadata.as_map().get("visible"),
		Some(&HistoryMetadataValue::Boolean(true)),
	);
	assert_eq!(
		json_item.metadata.as_map().get("note"),
		Some(&HistoryMetadataValue::Text("secret sauce".into())),
	);
	assert_eq!(
		json_item.metadata.as_map().get("summary"),
		Some(&HistoryMetadataValue::Text("token budget".into())),
	);
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
		Availability::Unavailable { reason: "typed PostgreSQL configuration is unavailable" }
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
async fn singleton_authority_precedes_identity_and_product_bootstrap() {
	let temp = TempDir::new().expect("singleton-order temp");
	let decodex_root = root(&temp);

	write_config(
		&decodex_root,
		&config(&temp.path().join("missing-postgres-socket"), "decodex", None),
	);

	let paths = decodex_root.paths();
	let authority = local_transport(&decodex_root);
	let listener = authority.bind().await.expect("hold singleton authority");

	assert!(!paths.server_identity_file().exists());

	let blocked = ServiceComposition::bootstrap(decodex_root).await;

	assert!(
		!paths.server_identity_file().exists(),
		"a blocked daemon must not create stable identity before singleton authority",
	);
	assert!(matches!(
		blocked.bind(ServerConfig::default()).await,
		Err(decodex_runtime::ServerError::LocalTransport(LocalTransportRefusal::EndpointInUse))
	));

	listener.cleanup().expect("release singleton fixture authority");
}

#[tokio::test]
async fn unsafe_and_malformed_host_configuration_fail_closed() {
	let unsafe_temp = TempDir::new().expect("unsafe-path temp");
	let unsafe_root = root(&unsafe_temp);
	let unsafe_config = config(Path::new("/tmp/../operator-private-postgres"), "decodex", None);

	write_config(&unsafe_root, &unsafe_config);

	let unsafe_bootstrap = ServiceComposition::bootstrap(unsafe_root).await;

	assert_eq!(
		status(&unsafe_bootstrap, DoctorComponent::ManagedRepository),
		DoctorStatus::Unavailable(DoctorIssue::DatabaseNotConfigured)
	);
	assert_eq!(
		status(&unsafe_bootstrap, DoctorComponent::ProductStore),
		DoctorStatus::Unavailable(DoctorIssue::UnsafeHostPath)
	);
	assert!(matches!(
		unsafe_bootstrap.product_state_availability(),
		Availability::Unavailable { .. }
	));

	#[cfg(unix)]
	{
		let symlink_temp = TempDir::new().expect("symlink-path temp");
		let symlink_root = root(&symlink_temp);
		let socket_target = symlink_temp.path().join("socket-target");
		let socket_link = symlink_temp.path().join("socket-link");

		fs::create_dir(&socket_target).expect("socket target");
		std::os::unix::fs::symlink(&socket_target, &socket_link).expect("socket symlink");

		write_config(&symlink_root, &config(&socket_link, "decodex", None));

		let symlink_bootstrap = ServiceComposition::bootstrap(symlink_root).await;

		assert_eq!(
			status(&symlink_bootstrap, DoctorComponent::ManagedRepository),
			DoctorStatus::Unavailable(DoctorIssue::DatabaseNotConfigured)
		);
		assert_eq!(
			status(&symlink_bootstrap, DoctorComponent::ProductStore),
			DoctorStatus::Unavailable(DoctorIssue::UnsafeHostPath)
		);
		assert!(matches!(
			symlink_bootstrap.product_state_availability(),
			Availability::Unavailable { .. }
		));

		let ancestor_temp = TempDir::new().expect("ancestor-symlink temp");
		let ancestor_root = root(&ancestor_temp);
		let ancestor_target = ancestor_temp.path().join("ancestor-target");
		let ancestor_link = ancestor_temp.path().join("ancestor-link");
		let nested_socket = ancestor_link.join("socket");

		fs::create_dir(&ancestor_target).expect("ancestor target");
		fs::create_dir(ancestor_target.join("socket")).expect("nested socket");
		std::os::unix::fs::symlink(&ancestor_target, &ancestor_link)
			.expect("ancestor directory symlink");

		write_config(&ancestor_root, &config(&nested_socket, "decodex", None));

		let ancestor_bootstrap = ServiceComposition::bootstrap(ancestor_root).await;

		assert_eq!(
			status(&ancestor_bootstrap, DoctorComponent::ManagedRepository),
			DoctorStatus::Unavailable(DoctorIssue::DatabaseNotConfigured)
		);
		assert_eq!(
			status(&ancestor_bootstrap, DoctorComponent::ProductStore),
			DoctorStatus::Unavailable(DoctorIssue::UnsafeHostPath)
		);
	}

	let invalid_postgres_temp = TempDir::new().expect("invalid-postgres temp");
	let invalid_postgres_root = root(&invalid_postgres_temp);
	let invalid_postgres =
		config(invalid_postgres_temp.path(), "decodex", None).replace("port = 5432", "port = 0");

	write_config(&invalid_postgres_root, &invalid_postgres);

	let invalid_postgres_bootstrap = ServiceComposition::bootstrap(invalid_postgres_root).await;

	assert_eq!(
		status(&invalid_postgres_bootstrap, DoctorComponent::Configuration),
		DoctorStatus::Unavailable(DoctorIssue::ConfigurationMalformed)
	);
	assert_eq!(
		status(&invalid_postgres_bootstrap, DoctorComponent::ProductStore),
		DoctorStatus::Unavailable(DoctorIssue::DatabaseMalformedConfig)
	);
}

#[cfg(unix)]
#[tokio::test]
async fn symlinked_owned_config_is_unsafe_not_malformed() {
	let temp = TempDir::new().expect("symlinked-config temp");
	let root = root(&temp);
	let paths = root.paths();
	let external = temp.path().join("external-config.toml");

	paths.ensure_layout().expect("create fixture layout");

	fs::write(&external, "version = 1\n").expect("write external config");
	std::os::unix::fs::symlink(&external, paths.config_file()).expect("symlink config fixture");

	let bootstrap = ServiceComposition::bootstrap(root).await;

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
async fn unreachable_authentication_and_unprobed_states_are_typed() {
	let unreachable_temp = TempDir::new().expect("unreachable-postgres temp");
	let unreachable_root = root(&unreachable_temp);
	let unreachable_host =
		unreachable_temp.path().canonicalize().expect("canonical unreachable host");
	let missing_socket = unreachable_host.join("missing-socket");

	write_config(&unreachable_root, &config(&missing_socket, "decodex", None));

	let unreachable = ServiceComposition::bootstrap(unreachable_root).await;

	assert_eq!(
		status(&unreachable, DoctorComponent::ProductStore),
		DoctorStatus::Unavailable(DoctorIssue::DatabaseUnreachable)
	);
	assert_eq!(status(&unreachable, DoctorComponent::Protocol), DoctorStatus::Ready);
	assert_eq!(status(&unreachable, DoctorComponent::ProtocolVersion), DoctorStatus::Ready);
	assert_eq!(status(&unreachable, DoctorComponent::ServerIdentity), DoctorStatus::Ready);
	assert_eq!(
		status(&unreachable, DoctorComponent::ManagedRepository),
		DoctorStatus::Unavailable(DoctorIssue::DatabaseNotConfigured)
	);
	assert_eq!(
		status(&unreachable, DoctorComponent::BlobIntegrity),
		DoctorStatus::Unknown(DoctorIssue::NotProbed)
	);
	assert_eq!(
		status(&unreachable, DoctorComponent::CredentialVault),
		DoctorStatus::Unknown(DoctorIssue::Authentication)
	);
	assert_eq!(
		status(&unreachable, DoctorComponent::PluginReadiness),
		DoctorStatus::Unknown(DoctorIssue::Plugin)
	);

	for capability in AppServerCapability::ALL {
		assert_eq!(
			status(&unreachable, DoctorComponent::AppServerCapability(capability)),
			DoctorStatus::Unknown(DoctorIssue::NotProbed)
		);
	}

	let authentication_temp = TempDir::new().expect("authentication temp");
	let authentication_root = root(&authentication_temp);
	let authentication_host =
		authentication_temp.path().canonicalize().expect("canonical authentication host");

	write_config(
		&authentication_root,
		&config(
			&authentication_host,
			"decodex",
			Some("DECODEX_XY_1307_DETERMINISTICALLY_MISSING_CREDENTIAL"),
		),
	);

	let authentication = ServiceComposition::bootstrap(authentication_root).await;

	assert_eq!(
		status(&authentication, DoctorComponent::ProductStore),
		DoctorStatus::Unavailable(DoctorIssue::Authentication)
	);
	assert_eq!(
		status(&authentication, DoctorComponent::CredentialVault),
		DoctorStatus::Unavailable(DoctorIssue::Authentication)
	);

	#[cfg(unix)]
	{
		let ancestor_temp = TempDir::new().expect("socket-ancestor temp");
		let ancestor_root = root(&ancestor_temp);
		let socket_target = ancestor_temp.path().join("socket-target");
		let socket_link = ancestor_temp.path().join("socket-link");
		let nested_socket = socket_link.join("socket");

		fs::create_dir(&socket_target).expect("socket ancestor target");
		fs::create_dir(socket_target.join("socket")).expect("nested socket directory");
		std::os::unix::fs::symlink(&socket_target, &socket_link).expect("socket ancestor symlink");

		write_config(&ancestor_root, &config(&nested_socket, "decodex", None));

		let ancestor = ServiceComposition::bootstrap(ancestor_root).await;

		assert_eq!(
			status(&ancestor, DoctorComponent::ProductStore),
			DoctorStatus::Unavailable(DoctorIssue::UnsafeHostPath)
		);
	}
}

#[tokio::test]
async fn doctor_crosses_the_daemon_protocol_and_wrong_server_is_refused() {
	let temp = TempDir::new().expect("protocol temp");
	let decodex_root = root(&temp);

	write_config(
		&decodex_root,
		&config(&temp.path().join("missing-postgres-socket"), "decodex", None),
	);

	let transport = local_transport(&decodex_root);
	let bootstrap = ServiceComposition::bootstrap(decodex_root).await;
	let server_id = bootstrap.server_id().clone();
	let mut bound = bootstrap.bind(ServerConfig::default()).await.expect("bind daemon fixture");
	let mut wrong = connect_local(&transport).await;

	send(
		&mut wrong,
		ClientMessage::Hello(ClientHello {
			version: CURRENT_VERSION,
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
			query_id: QueryId::new("history-unavailable").expect("test operation must succeed"),
			payload: QueryPayload::GetConversationHistory {
				conversation_id: EntityId::new("40000000-0000-4000-8000-000000000001")
					.expect("test operation must succeed"),
				after: None,
				page_size: 1,
			},
		}),
	)
	.await;

	assert!(
		matches!(receive(&mut client).await, ServerMessage::QueryResult(result) if matches!(result.payload,
		QueryResultPayload::ConversationHistory(ConversationHistoryResult::Unavailable { error: HistoryQueryError::ProductStateUnavailable })))
	);

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
			version: ProtocolVersion { major: 2, minor: 1 },
			expected_server_id: Some(server_id.clone()),
			resume: None,
		}),
	)
	.await;
	let ServerMessage::Refusal(refusal) = receive(&mut future).await else {
		panic!("expected V2.1 minor-version refusal");
	};
	assert!(matches!(
		refusal.refusal,
		Refusal::UnsupportedVersion(VersionRefusal::UnsupportedMinor { .. })
	));

	let mut current = connect_local(transport).await;

	send(
		&mut current,
		ClientMessage::Hello(ClientHello {
			version: CURRENT_VERSION,
			expected_server_id: Some(server_id.clone()),
			resume: None,
		}),
	)
	.await;

	assert!(matches!(receive(&mut current).await, ServerMessage::Welcome(_)));
	assert!(matches!(receive(&mut current).await, ServerMessage::Snapshot(_)));

	send(&mut current, ClientMessage::Query(doctor_query(CURRENT_VERSION, "current-doctor-query")))
		.await;

	let ServerMessage::QueryResult(result) = receive(&mut current).await else {
		panic!("expected current-version doctor result");
	};
	let QueryResultPayload::DoctorStatus(report) = result.payload else {
		panic!("expected doctor result");
	};

	assert_eq!(report.server_id(), server_id);
	assert_eq!(report.version(), CURRENT_VERSION);

	send(
		&mut current,
		ClientMessage::Query(doctor_query(
			ProtocolVersion { major: 2, minor: 1 },
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

	assert!(matches!(transport.connect().await, Err(LocalTransportRefusal::EndpointUnavailable),));
}

#[tokio::test]
#[ignore = "requires the isolated PostgreSQL 18 bootstrap harness"]
async fn isolated_postgres_bootstrap_is_available_through_the_daemon() {
	let root_path = PathBuf::from(
		env::var("DECODEX_TEST_BOOTSTRAP_ROOT").expect("isolated bootstrap root environment"),
	);
	let decodex_root = DecodexRoot::new(root_path).expect("isolated bootstrap root is safe");
	let transport = local_transport(&decodex_root);
	let blob_store = BlobStore::open(decodex_root.paths()).expect("open daemon blob store");
	let large_text = "large-history-payload-".repeat(900);
	let large_hash = blob_store.put(large_text.as_bytes()).expect("publish fixture blob");

	blob_store.put(b"artifact provenance bytes").expect("publish referenced Artifact fixture blob");
	blob_store
		.put(&b"writer-reclaimer-race".repeat(1_000))
		.expect("publish racing history fixture blob");

	let bootstrap = ServiceComposition::bootstrap(decodex_root).await;

	assert_eq!(status(&bootstrap, DoctorComponent::ProductStore), DoctorStatus::Ready);
	assert_eq!(
		status(&bootstrap, DoctorComponent::CredentialVault),
		DoctorStatus::Unavailable(DoctorIssue::Integrity)
	);
	assert_eq!(bootstrap.product_state_availability(), Availability::Available);

	let server_id = bootstrap.server_id().clone();
	let mut bound =
		bootstrap.bind(ServerConfig::default()).await.expect("bind PostgreSQL history daemon");
	let mut client = connect_local(&transport).await;

	send(
		&mut client,
		ClientMessage::Hello(ClientHello {
			version: CURRENT_VERSION,
			expected_server_id: Some(server_id),
			resume: None,
		}),
	)
	.await;

	assert!(matches!(receive(&mut client).await, ServerMessage::Welcome(_)));
	assert!(matches!(receive(&mut client).await, ServerMessage::Snapshot(_)));

	assert_postgres_history_queries(&mut client, &blob_store, large_hash).await;

	bound.shutdown().await.expect("shutdown PostgreSQL history daemon");
}

async fn assert_postgres_history_queries<S>(
	client: &mut WebSocketStream<S>,
	blob_store: &BlobStore,
	large_hash: BlobHash,
) where
	S: AsyncRead + AsyncWrite + Unpin,
{
	send(client, history_query("history-first-page", None, 4)).await;

	let ServerMessage::QueryResult(result) = receive(client).await else {
		panic!("expected first history page");
	};
	let QueryResultPayload::ConversationHistory(ConversationHistoryResult::Page(first_page)) =
		result.payload
	else {
		panic!("expected first history page payload");
	};
	let issued_cursor =
		first_page.next_cursor.as_ref().expect("issued continuation").as_str().to_owned();

	assert_history_projection(&first_page);
	send(client, history_query("history-success", Some(&issued_cursor), 4)).await;

	let artifact_response = receive(client).await;
	let ServerMessage::QueryResult(result) = artifact_response else {
		panic!("expected continuation result: {artifact_response:?}");
	};
	let QueryResultPayload::ConversationHistory(ConversationHistoryResult::Page(page)) =
		result.payload
	else {
		panic!("expected Artifact history page: {:?}", result.payload);
	};
	let item = page.items.first().expect("Artifact history item");
	let artifact = item.artifact.as_ref().expect("typed Artifact reference");

	assert_eq!(item.kind, HistoryItemKindDto::Artifact);
	assert_eq!(artifact.artifact_id.as_str(), "48000000-0000-4000-8000-000000000001");
	assert_eq!(artifact.revision.get(), 1);

	send(client, history_query("history-size-mismatch", Some(&issued_cursor), 1)).await;

	assert!(
		matches!(receive(client).await,ServerMessage::QueryResult(result) if matches!(result.payload,QueryResultPayload::ConversationHistory(ConversationHistoryResult::Unavailable{error:HistoryQueryError::InvalidRequest})))
	);

	expire_isolated_history_cursor(&issued_cursor);
	send(client, history_query("history-expired", Some(&issued_cursor), 4)).await;

	assert!(
		matches!(receive(client).await,ServerMessage::QueryResult(result) if matches!(result.payload,QueryResultPayload::ConversationHistory(ConversationHistoryResult::Unavailable{error:HistoryQueryError::InvalidRequest})))
	);

	assert_postgres_cursor_capacity(client).await;
	send(client, history_query("history-bound", None, MAX_HISTORY_PAGE_SIZE + 1)).await;

	assert!(
		matches!(receive(client).await,ServerMessage::QueryResult(result) if matches!(result.payload,QueryResultPayload::ConversationHistory(ConversationHistoryResult::Unavailable{error:HistoryQueryError::InvalidRequest})))
	);

	send(client, history_query("history-malformed", Some("x"), 1)).await;

	assert!(
		matches!(receive(client).await,ServerMessage::QueryResult(result) if matches!(result.payload,QueryResultPayload::ConversationHistory(ConversationHistoryResult::Unavailable{error:HistoryQueryError::InvalidRequest})))
	);

	for (query_id, cursor) in [
		("history-never-issued", "v1:44000000-0000-4000-8000-000000000099"),
		("history-edited-boundary", "v1:44000000-0000-4000-8000-000000000098:1"),
	] {
		send(client, history_query(query_id, Some(cursor), 1)).await;

		assert!(
			matches!(receive(client).await,ServerMessage::QueryResult(result) if matches!(result.payload,QueryResultPayload::ConversationHistory(ConversationHistoryResult::Unavailable{error:HistoryQueryError::InvalidRequest})))
		);
	}

	// The PostgreSQL contract attempted and rejected an invalid media-type write in this same
	// isolated database. A fresh first-page query remains readable rather than integrity-poisoned.
	send(client, history_query("history-still-readable-after-invalid-media", None, 1)).await;

	let readable = receive(client).await;
	let ServerMessage::QueryResult(readable) = readable else {
		panic!("valid history was poisoned after rejected media: {readable:?}");
	};
	let payload = readable.payload;
	let QueryResultPayload::ConversationHistory(ConversationHistoryResult::Page(readable)) =
		payload
	else {
		panic!("valid history was poisoned after rejected media: {payload:?}");
	};
	let fresh_cursor = readable.next_cursor.expect("fresh readable page has a continuation");

	send(
		client,
		history_query_for(
			"history-cross",
			"40000000-0000-4000-8000-000000000099",
			Some(fresh_cursor.as_str()),
			1,
		),
	)
	.await;

	assert!(
		matches!(receive(client).await,ServerMessage::QueryResult(result) if matches!(result.payload,QueryResultPayload::ConversationHistory(ConversationHistoryResult::Unavailable{error:HistoryQueryError::InvalidRequest})))
	);

	fs::write(blob_store.path_for(large_hash), b"tampered").expect("tamper history blob");

	send(client, history_query("history-tampered", None, 1)).await;

	assert!(
		matches!(receive(client).await,ServerMessage::QueryResult(result) if matches!(result.payload,QueryResultPayload::ConversationHistory(ConversationHistoryResult::Unavailable{error:HistoryQueryError::IntegrityUnavailable})))
	);

	fs::remove_file(blob_store.path_for(large_hash)).expect("remove tampered history blob");

	send(client, history_query("history-missing", None, 1)).await;

	assert!(
		matches!(receive(client).await,ServerMessage::QueryResult(result) if matches!(result.payload,QueryResultPayload::ConversationHistory(ConversationHistoryResult::Unavailable{error:HistoryQueryError::IntegrityUnavailable})))
	);
}

async fn assert_postgres_cursor_capacity<S>(client: &mut WebSocketStream<S>)
where
	S: AsyncRead + AsyncWrite + Unpin,
{
	exhaust_isolated_history_cursor_capacity();
	send(
		client,
		history_query_for(
			"history-resource-exhausted",
			"49000000-0000-4000-8000-000000000010",
			None,
			8,
		),
	)
	.await;

	assert!(
		matches!(receive(client).await,ServerMessage::QueryResult(result) if matches!(result.payload,QueryResultPayload::ConversationHistory(ConversationHistoryResult::Unavailable{error:HistoryQueryError::ResourceExhausted})))
	);

	run_isolated_sql(
		"UPDATE decodex.history_cursors \
		 SET created_at=clock_timestamp()-interval '2 hours', \
		 expires_at=clock_timestamp()-interval '1 hour' \
		 WHERE conversation_id='49000000-0000-4000-8000-000000000010'",
	);
}

#[cfg(unix)]
#[tokio::test]
#[ignore = "requires the isolated PostgreSQL 18 bootstrap harness"]
async fn isolated_postgres_live_doctor_rejects_replaced_endpoint() {
	let root_path = PathBuf::from(
		env::var("DECODEX_TEST_BOOTSTRAP_ROOT").expect("isolated bootstrap root environment"),
	);
	let socket_directory = PathBuf::from(
		env::var("DECODEX_TEST_SOCKET_DIRECTORY").expect("isolated socket directory environment"),
	);
	let port = env::var("DECODEX_TEST_SOCKET_PORT")
		.expect("isolated socket port environment")
		.parse::<u16>()
		.expect("isolated socket port is valid");
	let decodex_root = DecodexRoot::new(root_path).expect("isolated bootstrap root is safe");
	let transport = local_transport(&decodex_root);
	let bootstrap = ServiceComposition::bootstrap(decodex_root).await;

	assert_eq!(status(&bootstrap, DoctorComponent::ProductStore), DoctorStatus::Ready);

	let server_id = bootstrap.server_id().clone();
	let mut bound =
		bootstrap.bind(ServerConfig::default()).await.expect("bind live-doctor daemon fixture");
	let mut client = connect_local(&transport).await;

	send(
		&mut client,
		ClientMessage::Hello(ClientHello {
			version: CURRENT_VERSION,
			expected_server_id: Some(server_id),
			resume: None,
		}),
	)
	.await;

	assert!(matches!(receive(&mut client).await, ServerMessage::Welcome(_)));
	assert!(matches!(receive(&mut client).await, ServerMessage::Snapshot(_)));

	send(&mut client, ClientMessage::Query(doctor_query(CURRENT_VERSION, "live-doctor-query")))
		.await;

	let ServerMessage::QueryResult(ready_result) = receive(&mut client).await else {
		panic!("expected live doctor result before endpoint replacement");
	};
	let QueryResultPayload::DoctorStatus(ready_report) = ready_result.payload else {
		panic!("expected doctor result");
	};

	assert_eq!(
		ready_report
			.check(DoctorComponent::ProductStore)
			.expect("database check is present")
			.status,
		DoctorStatus::Ready
	);

	{
		let _replacement = SocketDirectoryReplacement::install(socket_directory, port);

		send(&mut client, ClientMessage::Query(doctor_query(CURRENT_VERSION, "live-doctor-query")))
			.await;

		let ServerMessage::QueryResult(result) = receive(&mut client).await else {
			panic!("expected live doctor result after endpoint replacement");
		};
		let QueryResultPayload::DoctorStatus(report) = result.payload else {
			panic!("expected doctor result");
		};

		assert_eq!(
			report.check(DoctorComponent::ProductStore).expect("database check is present").status,
			DoctorStatus::Unavailable(DoctorIssue::UnsafeHostPath)
		);
	}

	drop(client);

	bound.shutdown().await.expect("shutdown live-doctor daemon fixture");
}

#[tokio::test]
#[ignore = "requires the isolated PostgreSQL 18 bootstrap harness"]
async fn isolated_postgres_live_doctor_detects_database_drift() {
	let root_path = PathBuf::from(
		env::var("DECODEX_TEST_LIVE_INCOMPATIBLE_ROOT")
			.expect("live-incompatible bootstrap root environment"),
	);
	let sync = PathBuf::from(
		env::var("DECODEX_TEST_LIVE_INCOMPATIBLE_SYNC")
			.expect("live-incompatible synchronization environment"),
	);
	let decodex_root = DecodexRoot::new(root_path).expect("live-incompatible root is safe");
	let transport = local_transport(&decodex_root);
	let bootstrap = ServiceComposition::bootstrap(decodex_root).await;

	assert_eq!(status(&bootstrap, DoctorComponent::ProductStore), DoctorStatus::Ready);

	let server_id = bootstrap.server_id().clone();
	let mut bound = bootstrap
		.bind(ServerConfig::default())
		.await
		.expect("bind live-incompatible daemon fixture");
	let mut client = connect_local(&transport).await;

	send(
		&mut client,
		ClientMessage::Hello(ClientHello {
			version: CURRENT_VERSION,
			expected_server_id: Some(server_id),
			resume: None,
		}),
	)
	.await;

	assert!(matches!(receive(&mut client).await, ServerMessage::Welcome(_)));
	assert!(matches!(receive(&mut client).await, ServerMessage::Snapshot(_)));

	send(&mut client, ClientMessage::Query(doctor_query(CURRENT_VERSION, "live-state"))).await;

	let ServerMessage::QueryResult(ready) = receive(&mut client).await else {
		panic!("expected ready live database observation");
	};
	let QueryResultPayload::DoctorStatus(ready) = ready.payload else {
		panic!("expected doctor result");
	};

	assert_eq!(
		ready.check(DoctorComponent::ProductStore).expect("database check").status,
		DoctorStatus::Ready
	);

	fs::write(sync.join("ready"), b"ready").expect("publish live-doctor readiness barrier");
	time::timeout(Duration::from_secs(15), async {
		while !sync.join("mutated").exists() {
			time::sleep(Duration::from_millis(10)).await;
		}
	})
	.await
	.expect("database mutation barrier timed out");

	send(&mut client, ClientMessage::Query(doctor_query(CURRENT_VERSION, "live-state"))).await;

	let ServerMessage::QueryResult(changed) = receive(&mut client).await else {
		panic!("expected changed live database observation");
	};
	let QueryResultPayload::DoctorStatus(changed) = changed.payload else {
		panic!("expected doctor result");
	};

	assert_eq!(
		changed.check(DoctorComponent::ProductStore).expect("database check").status,
		DoctorStatus::Unavailable(if env::var_os("DECODEX_TEST_LIVE_EXPECTED_UNSAFE").is_some() {
			DoctorIssue::UnsafeDatabaseAuthority
		} else {
			DoctorIssue::DatabaseIncompatible
		},)
	);

	drop(client);

	bound.shutdown().await.expect("shutdown live-incompatible daemon fixture");
}

#[tokio::test]
#[ignore = "requires the isolated PostgreSQL 18 bootstrap harness"]
async fn isolated_postgres_rejected_role_is_authentication() {
	let root_path = PathBuf::from(
		env::var("DECODEX_TEST_AUTH_BOOTSTRAP_ROOT")
			.expect("isolated authentication bootstrap root environment"),
	);
	let bootstrap = ServiceComposition::bootstrap(
		DecodexRoot::new(root_path).expect("isolated authentication root is safe"),
	)
	.await;

	assert_eq!(
		status(&bootstrap, DoctorComponent::ProductStore),
		DoctorStatus::Unavailable(DoctorIssue::Authentication)
	);
}

#[tokio::test]
#[ignore = "requires the isolated PostgreSQL 18 bootstrap harness"]
async fn isolated_postgres_overprivileged_runtime_is_unavailable() {
	let case_id = "truncate";
	let root_path = PathBuf::from(
		env::var("DECODEX_TEST_UNSAFE_AUTHORITY_ROOT")
			.expect("representative unsafe-authority root environment"),
	);
	let bootstrap = ServiceComposition::bootstrap(
		DecodexRoot::new(root_path).expect("representative unsafe-authority root is safe"),
	)
	.await;
	let actual_status = status(&bootstrap, DoctorComponent::ProductStore);
	let actual_availability = bootstrap.product_state_availability();
	let expected_status = DoctorStatus::Unavailable(DoctorIssue::UnsafeDatabaseAuthority);
	let expected_availability =
		Availability::Unavailable { reason: "configured PostgreSQL runtime authority is unsafe" };

	assert_eq!(
		actual_status, expected_status,
		"authority projection {case_id}: expected status {expected_status:?}, actual status \
		 {actual_status:?}"
	);
	assert_eq!(
		actual_availability, expected_availability,
		"authority projection {case_id}: expected availability {expected_availability:?}, \
		 actual availability {actual_availability:?}"
	);
}

#[tokio::test]
#[ignore = "requires the isolated PostgreSQL 18 bootstrap harness"]
async fn isolated_postgres_incompatible_runtime_is_unavailable() {
	let case_id = "missing-ledger-select";
	let root_path = PathBuf::from(
		env::var("DECODEX_TEST_INCOMPATIBLE_AUTHORITY_ROOT")
			.expect("representative incompatible-authority root environment"),
	);
	let bootstrap = ServiceComposition::bootstrap(
		DecodexRoot::new(root_path).expect("representative incompatible-authority root is safe"),
	)
	.await;
	let actual_status = status(&bootstrap, DoctorComponent::ProductStore);
	let actual_availability = bootstrap.product_state_availability();
	let expected_status = DoctorStatus::Unavailable(DoctorIssue::DatabaseIncompatible);
	let expected_availability =
		Availability::Unavailable { reason: "configured PostgreSQL is incompatible" };

	assert_eq!(
		actual_status, expected_status,
		"authority projection {case_id}: expected status {expected_status:?}, actual status \
		 {actual_status:?}"
	);
	assert_eq!(
		actual_availability, expected_availability,
		"authority projection {case_id}: expected availability {expected_availability:?}, \
		 actual availability {actual_availability:?}"
	);
}

#[tokio::test]
#[ignore = "requires the isolated PostgreSQL 18 bootstrap harness"]
async fn isolated_postgres_hostile_search_path_is_unavailable() {
	let root_path = PathBuf::from(
		env::var("DECODEX_TEST_HOSTILE_SEARCH_ROOT")
			.expect("isolated hostile-search root environment"),
	);
	let bootstrap = ServiceComposition::bootstrap(
		DecodexRoot::new(root_path).expect("isolated hostile-search root is safe"),
	)
	.await;

	assert_eq!(
		status(&bootstrap, DoctorComponent::ProductStore),
		DoctorStatus::Unavailable(DoctorIssue::UnsafeDatabaseAuthority)
	);
	assert_eq!(
		bootstrap.product_state_availability(),
		Availability::Unavailable { reason: "configured PostgreSQL runtime authority is unsafe" }
	);
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
