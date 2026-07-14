//! XY-1307 typed fail-closed bootstrap and authoritative doctor protocol fixtures.
#![allow(unused_crate_dependencies)]

#[cfg(unix)]
use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _};
#[cfg(unix)] use std::os::unix::net::UnixListener;
use std::{
	env,
	fs::{self, OpenOptions, Permissions},
	io::Write as _,
	net::{Ipv4Addr, SocketAddr},
	path::{Path, PathBuf},
	process,
	time::Duration,
};

use futures_util::{SinkExt as _, StreamExt as _};
use tempfile::TempDir;
use tokio::{
	io::{AsyncRead, AsyncWrite},
	net::TcpListener,
	time,
};
use tokio_tungstenite::{WebSocketStream, tungstenite::Message};

use decodex_core::{Availability, DecodexRoot};
use decodex_protocol::{
	AppServerCapability, CURRENT_VERSION, ClientHello, ClientMessage, DoctorComponent, DoctorIssue,
	DoctorStatus, PREVIOUS_MINOR_VERSION, ProtocolVersion, QueryEnvelope, QueryId, QueryPayload,
	QueryResultPayload, Refusal, ServerId, ServerMessage,
};
use decodex_runtime::{ServerConfig, ServiceBootstrap, ServiceComposition};

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

fn config(repository: &Path, socket: &Path, database: &str, credential: Option<&str>) -> String {
	let migration_credential = credential
		.map(|name| format!("credential_env_var = \"{name}_MIGRATION\"\n"))
		.unwrap_or_default();
	let runtime_credential = credential
		.map(|name| format!("credential_env_var = \"{name}_RUNTIME\"\n"))
		.unwrap_or_default();
	let fixture_user = env::var("USER").unwrap_or_else(|_| "postgres".into());

	format!(
		r#"version = 1
active_profile = "local"

[profiles.local]
kind = "local"
address = "127.0.0.1:49152"

[server_host.repositories.fixture]
host_path = "{}"

[postgres]
socket_directory = "{}"
expected_peer_uid = {}
port = 5432
database = "{}"

[postgres.migration]
user = "{}_migration"
{}
[postgres.runtime]
user = "{}_runtime"
{}
[cache]
max_entries = 16
max_bytes = 65536
max_entry_bytes = 4096
"#,
		repository.display(),
		socket.display(),
		env::current_dir()
			.expect("fixture current directory")
			.metadata()
			.expect("fixture owner metadata")
			.uid(),
		database,
		fixture_user,
		migration_credential,
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

#[tokio::test]
async fn missing_malformed_and_redacted_bootstrap_are_typed() {
	let missing_temp = TempDir::new().expect("missing-config temp");
	let missing = ServiceComposition::bootstrap(root(&missing_temp)).await;

	assert_eq!(
		status(&missing, DoctorComponent::Configuration),
		DoctorStatus::Unavailable(DoctorIssue::ConfigurationMissing)
	);
	assert_eq!(
		status(&missing, DoctorComponent::Database),
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
		status(&malformed, DoctorComponent::Database),
		DoctorStatus::Unavailable(DoctorIssue::DatabaseMalformedConfig)
	);
	assert!(!encoded.contains(secret));
	assert!(!encoded.contains(malformed_temp.path().to_string_lossy().as_ref()));
}

#[tokio::test]
async fn unsafe_and_malformed_host_configuration_fail_closed() {
	let unsafe_temp = TempDir::new().expect("unsafe-path temp");
	let unsafe_root = root(&unsafe_temp);
	let unsafe_config = config(
		Path::new("/tmp/../operator-private-repository"),
		unsafe_temp.path(),
		"decodex",
		None,
	);

	write_config(&unsafe_root, &unsafe_config);

	let unsafe_bootstrap = ServiceComposition::bootstrap(unsafe_root).await;

	assert_eq!(
		status(&unsafe_bootstrap, DoctorComponent::ServerRepositories),
		DoctorStatus::Unavailable(DoctorIssue::UnsafeHostPath)
	);
	assert_eq!(
		status(&unsafe_bootstrap, DoctorComponent::Database),
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
		let repository_target = symlink_temp.path().join("repository-target");
		let repository_link = symlink_temp.path().join("repository-link");

		fs::create_dir(&repository_target).expect("repository target");
		std::os::unix::fs::symlink(&repository_target, &repository_link)
			.expect("repository symlink");

		write_config(
			&symlink_root,
			&config(&repository_link, symlink_temp.path(), "decodex", None),
		);

		let symlink_bootstrap = ServiceComposition::bootstrap(symlink_root).await;

		assert_eq!(
			status(&symlink_bootstrap, DoctorComponent::ServerRepositories),
			DoctorStatus::Unavailable(DoctorIssue::UnsafeHostPath)
		);
		assert_eq!(
			status(&symlink_bootstrap, DoctorComponent::Database),
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
		let nested_repository = ancestor_link.join("repository");

		fs::create_dir(&ancestor_target).expect("ancestor target");
		fs::create_dir(ancestor_target.join("repository")).expect("nested repository");
		std::os::unix::fs::symlink(&ancestor_target, &ancestor_link)
			.expect("ancestor directory symlink");

		write_config(
			&ancestor_root,
			&config(&nested_repository, ancestor_temp.path(), "decodex", None),
		);

		let ancestor_bootstrap = ServiceComposition::bootstrap(ancestor_root).await;

		assert_eq!(
			status(&ancestor_bootstrap, DoctorComponent::ServerRepositories),
			DoctorStatus::Unavailable(DoctorIssue::UnsafeHostPath)
		);
		assert_eq!(
			status(&ancestor_bootstrap, DoctorComponent::Database),
			DoctorStatus::Unavailable(DoctorIssue::UnsafeHostPath)
		);
	}

	let invalid_postgres_temp = TempDir::new().expect("invalid-postgres temp");
	let invalid_postgres_root = root(&invalid_postgres_temp);
	let invalid_postgres =
		config(invalid_postgres_temp.path(), invalid_postgres_temp.path(), "decodex", None)
			.replace("port = 5432", "port = 0");

	write_config(&invalid_postgres_root, &invalid_postgres);

	let invalid_postgres_bootstrap = ServiceComposition::bootstrap(invalid_postgres_root).await;

	assert_eq!(
		status(&invalid_postgres_bootstrap, DoctorComponent::Configuration),
		DoctorStatus::Unavailable(DoctorIssue::ConfigurationMalformed)
	);
	assert_eq!(
		status(&invalid_postgres_bootstrap, DoctorComponent::Database),
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
		status(&bootstrap, DoctorComponent::Database),
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

	write_config(&unreachable_root, &config(&unreachable_host, &missing_socket, "decodex", None));

	let unreachable = ServiceComposition::bootstrap(unreachable_root).await;

	assert_eq!(
		status(&unreachable, DoctorComponent::Database),
		DoctorStatus::Unavailable(DoctorIssue::DatabaseUnreachable)
	);
	assert_eq!(status(&unreachable, DoctorComponent::Protocol), DoctorStatus::Ready);
	assert_eq!(status(&unreachable, DoctorComponent::ProtocolVersion), DoctorStatus::Ready);
	assert_eq!(status(&unreachable, DoctorComponent::ServerIdentity), DoctorStatus::Ready);
	assert_eq!(status(&unreachable, DoctorComponent::ServerRepositories), DoctorStatus::Ready);
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
			&authentication_host,
			"decodex",
			Some("DECODEX_XY_1307_DETERMINISTICALLY_MISSING_CREDENTIAL"),
		),
	);

	let authentication = ServiceComposition::bootstrap(authentication_root).await;

	assert_eq!(
		status(&authentication, DoctorComponent::Database),
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

		write_config(
			&ancestor_root,
			&config(ancestor_temp.path(), &nested_socket, "decodex", None),
		);

		let ancestor = ServiceComposition::bootstrap(ancestor_root).await;

		assert_eq!(
			status(&ancestor, DoctorComponent::Database),
			DoctorStatus::Unavailable(DoctorIssue::UnsafeHostPath)
		);
	}
}

#[tokio::test]
async fn doctor_crosses_the_daemon_protocol_and_wrong_server_is_refused() {
	let temp = TempDir::new().expect("protocol temp");
	let bootstrap = ServiceComposition::bootstrap(root(&temp)).await;
	let server_id = bootstrap.server_id().clone();
	let bound = bootstrap
		.bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)), ServerConfig::default())
		.await
		.expect("bind daemon fixture");
	let url = format!("ws://{}/v1/ws", bound.address());
	let (mut wrong, _) = tokio_tungstenite::connect_async(&url).await.expect("connect wrong pin");

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

	let (mut client, _) =
		tokio_tungstenite::connect_async(&url).await.expect("connect matching pin");

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
	let QueryResultPayload::DoctorStatus(report) = result.payload;

	assert_eq!(report.server_id(), &server_id);
	assert_eq!(report.version(), CURRENT_VERSION);

	assert_cross_version_doctor_queries(&url, &server_id).await;
	drop((wrong, client));

	bound.shutdown().await.expect("shutdown daemon fixture");
}

async fn assert_cross_version_doctor_queries(url: &str, server_id: &ServerId) {
	let (mut previous, _) =
		tokio_tungstenite::connect_async(url).await.expect("connect previous-minor client");

	send(
		&mut previous,
		ClientMessage::Hello(ClientHello {
			version: PREVIOUS_MINOR_VERSION,
			expected_server_id: Some(server_id.clone()),
			resume: None,
		}),
	)
	.await;

	assert!(matches!(receive(&mut previous).await, ServerMessage::Welcome(_)));
	assert!(matches!(receive(&mut previous).await, ServerMessage::Snapshot(_)));

	send(
		&mut previous,
		ClientMessage::Query(doctor_query(PREVIOUS_MINOR_VERSION, "previous-doctor-query")),
	)
	.await;

	let ServerMessage::Refusal(result) = receive(&mut previous).await else {
		panic!("expected previous-minor doctor rejection");
	};

	assert!(matches!(result.refusal, Refusal::ProtocolViolation { .. }));

	send(
		&mut previous,
		ClientMessage::Query(doctor_query(PREVIOUS_MINOR_VERSION, "previous-doctor-query")),
	)
	.await;

	let ServerMessage::Refusal(result) = receive(&mut previous).await else {
		panic!("expected inverse previous-minor doctor rejection");
	};

	assert!(matches!(result.refusal, Refusal::ProtocolViolation { .. }));

	let (mut current, _) =
		tokio_tungstenite::connect_async(url).await.expect("connect inverse current client");

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
		panic!("expected inverse current-version doctor result");
	};
	let QueryResultPayload::DoctorStatus(report) = result.payload;

	assert_eq!(report.server_id(), server_id);
	assert_eq!(report.version(), CURRENT_VERSION);
}

#[tokio::test]
async fn disconnected_fixture_is_deterministic() {
	let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
		.await
		.expect("bind disconnected fixture");
	let address = listener.local_addr().expect("disconnected fixture address");

	drop(listener);

	assert!(tokio_tungstenite::connect_async(format!("ws://{address}/v1/ws")).await.is_err());
}

#[tokio::test]
#[ignore = "requires the isolated PostgreSQL 18 bootstrap harness"]
async fn isolated_postgres_bootstrap_is_available_through_the_daemon() {
	let root_path = PathBuf::from(
		env::var("DECODEX_TEST_BOOTSTRAP_ROOT").expect("isolated bootstrap root environment"),
	);
	let bootstrap = ServiceComposition::bootstrap(
		DecodexRoot::new(root_path).expect("isolated bootstrap root is safe"),
	)
	.await;

	assert_eq!(status(&bootstrap, DoctorComponent::Database), DoctorStatus::Ready);
	assert_eq!(
		status(&bootstrap, DoctorComponent::CredentialVault),
		DoctorStatus::Unknown(DoctorIssue::NotProbed)
	);
	assert_eq!(bootstrap.product_state_availability(), Availability::Available);
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
	let bootstrap = ServiceComposition::bootstrap(
		DecodexRoot::new(root_path).expect("isolated bootstrap root is safe"),
	)
	.await;

	assert_eq!(status(&bootstrap, DoctorComponent::Database), DoctorStatus::Ready);

	let server_id = bootstrap.server_id().clone();
	let bound = bootstrap
		.bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)), ServerConfig::default())
		.await
		.expect("bind live-doctor daemon fixture");
	let url = format!("ws://{}/v1/ws", bound.address());
	let (mut client, _) =
		tokio_tungstenite::connect_async(&url).await.expect("connect live-doctor client");

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
	let QueryResultPayload::DoctorStatus(ready_report) = ready_result.payload;

	assert_eq!(
		ready_report.check(DoctorComponent::Database).expect("database check is present").status,
		DoctorStatus::Ready
	);

	{
		let _replacement = SocketDirectoryReplacement::install(socket_directory, port);

		send(&mut client, ClientMessage::Query(doctor_query(CURRENT_VERSION, "live-doctor-query")))
			.await;

		let ServerMessage::QueryResult(result) = receive(&mut client).await else {
			panic!("expected live doctor result after endpoint replacement");
		};
		let QueryResultPayload::DoctorStatus(report) = result.payload;

		assert_eq!(
			report.check(DoctorComponent::Database).expect("database check is present").status,
			DoctorStatus::Unavailable(DoctorIssue::UnsafeHostPath)
		);
	}

	drop(client);

	bound.shutdown().await.expect("shutdown live-doctor daemon fixture");
}

#[tokio::test]
#[ignore = "requires the isolated PostgreSQL 18 bootstrap harness"]
async fn isolated_postgres_live_doctor_detects_database_incompatibility() {
	let root_path = PathBuf::from(
		env::var("DECODEX_TEST_LIVE_INCOMPATIBLE_ROOT")
			.expect("live-incompatible bootstrap root environment"),
	);
	let sync = PathBuf::from(
		env::var("DECODEX_TEST_LIVE_INCOMPATIBLE_SYNC")
			.expect("live-incompatible synchronization environment"),
	);
	let bootstrap = ServiceComposition::bootstrap(
		DecodexRoot::new(root_path).expect("live-incompatible root is safe"),
	)
	.await;

	assert_eq!(status(&bootstrap, DoctorComponent::Database), DoctorStatus::Ready);

	let server_id = bootstrap.server_id().clone();
	let bound = bootstrap
		.bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)), ServerConfig::default())
		.await
		.expect("bind live-incompatible daemon fixture");
	let url = format!("ws://{}/v1/ws", bound.address());
	let (mut client, _) =
		tokio_tungstenite::connect_async(&url).await.expect("connect live-incompatible client");

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
	let QueryResultPayload::DoctorStatus(ready) = ready.payload;

	assert_eq!(
		ready.check(DoctorComponent::Database).expect("database check").status,
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
	let QueryResultPayload::DoctorStatus(changed) = changed.payload;

	assert_eq!(
		changed.check(DoctorComponent::Database).expect("database check").status,
		DoctorStatus::Unavailable(DoctorIssue::DatabaseIncompatible)
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
		status(&bootstrap, DoctorComponent::Database),
		DoctorStatus::Unavailable(DoctorIssue::Authentication)
	);
}

#[tokio::test]
#[ignore = "requires the isolated PostgreSQL 18 bootstrap harness"]
async fn isolated_postgres_overprivileged_runtime_is_unavailable() {
	let roots = env::split_paths(
		&env::var_os("DECODEX_TEST_UNSAFE_AUTHORITY_ROOTS")
			.expect("isolated unsafe-authority roots environment"),
	)
	.collect::<Vec<_>>();

	assert_eq!(roots.len(), 27);

	for root_path in roots {
		let bootstrap = ServiceComposition::bootstrap(
			DecodexRoot::new(root_path).expect("isolated unsafe-authority root is safe"),
		)
		.await;

		assert_eq!(
			status(&bootstrap, DoctorComponent::Database),
			DoctorStatus::Unavailable(DoctorIssue::UnsafeDatabaseAuthority)
		);
		assert_eq!(
			bootstrap.product_state_availability(),
			Availability::Unavailable {
				reason: "configured PostgreSQL runtime authority is unsafe"
			}
		);
	}
}

#[tokio::test]
#[ignore = "requires the isolated PostgreSQL 18 bootstrap harness"]
async fn isolated_postgres_incompatible_runtime_is_unavailable() {
	let roots = env::split_paths(
		&env::var_os("DECODEX_TEST_INCOMPATIBLE_AUTHORITY_ROOTS")
			.expect("isolated incompatible-authority roots environment"),
	)
	.collect::<Vec<_>>();

	assert_eq!(roots.len(), 6);

	for root_path in roots {
		let bootstrap = ServiceComposition::bootstrap(
			DecodexRoot::new(root_path).expect("isolated incompatible-authority root is safe"),
		)
		.await;

		assert_eq!(
			status(&bootstrap, DoctorComponent::Database),
			DoctorStatus::Unavailable(DoctorIssue::DatabaseIncompatible)
		);
		assert_eq!(
			bootstrap.product_state_availability(),
			Availability::Unavailable { reason: "configured PostgreSQL is incompatible" }
		);
	}
}

#[tokio::test]
#[ignore = "requires the isolated PostgreSQL 18 bootstrap harness"]
async fn isolated_postgres_hostile_search_path_is_available() {
	let root_path = PathBuf::from(
		env::var("DECODEX_TEST_HOSTILE_SEARCH_ROOT")
			.expect("isolated hostile-search root environment"),
	);
	let bootstrap = ServiceComposition::bootstrap(
		DecodexRoot::new(root_path).expect("isolated hostile-search root is safe"),
	)
	.await;

	assert_eq!(status(&bootstrap, DoctorComponent::Database), DoctorStatus::Ready);
	assert_eq!(bootstrap.product_state_availability(), Availability::Available);

	let server_id = bootstrap.server_id().clone();
	let bound = bootstrap
		.bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)), ServerConfig::default())
		.await
		.expect("bind hostile-search daemon fixture");
	let url = format!("ws://{}/v1/ws", bound.address());
	let (mut client, _) =
		tokio_tungstenite::connect_async(&url).await.expect("connect hostile-search client");

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

	send(
		&mut client,
		ClientMessage::Query(doctor_query(CURRENT_VERSION, "hostile-catalog-doctor")),
	)
	.await;

	let ServerMessage::QueryResult(result) = receive(&mut client).await else {
		panic!("expected hostile-search doctor result");
	};
	let QueryResultPayload::DoctorStatus(report) = result.payload;

	assert_eq!(
		report.check(DoctorComponent::Database).expect("database status is present").status,
		DoctorStatus::Ready
	);

	drop(client);

	bound.shutdown().await.expect("shutdown hostile-search daemon fixture");
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
