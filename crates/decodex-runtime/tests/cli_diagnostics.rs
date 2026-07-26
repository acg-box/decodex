//! Real-process XY-1308 CLI/server diagnostic fixture matrix.
#![cfg(unix)]
#![allow(unused_crate_dependencies)]

use std::{
	env,
	fs::{self, OpenOptions, Permissions},
	io::Write as _,
	net::{Ipv4Addr, SocketAddr},
	os::unix::fs::{MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _},
	path::{Path, PathBuf},
	process::{Command, Output},
	time::Duration,
};

use futures_util::StreamExt as _;
use serde_json::Value;
use tempfile::TempDir;
use tokio::{task::JoinHandle, time};
use tokio_tungstenite::{self, tungstenite::Message};

use decodex_core::DecodexRoot;
use decodex_protocol::CURRENT_VERSION;
use decodex_runtime::{ServerConfig, ServiceComposition};

const SERVER_ID: &str = "018f0f9e-7b6e-4a31-8f4c-1d2e3f405162";
const WRONG_SERVER_ID: &str = "128f0f9e-7b6e-4a31-8f4c-1d2e3f405162";
const SECRET_MARKER: &str = "xy1308-secret-must-never-appear";
const SERVER_PATH_MARKER: &str = "xy1308-server-path-must-never-appear";
const FIXTURE_TIMEOUT: Duration = Duration::from_secs(10);

struct Fixture {
	_temp: TempDir,
	root: PathBuf,
	repository: PathBuf,
}
impl Fixture {
	fn new() -> Self {
		let temp = TempDir::new().expect("fixture temp");
		let canonical = temp.path().canonicalize().expect("canonical fixture temp");
		let root = canonical.join(".decodex");
		let repository = canonical.join(SERVER_PATH_MARKER);

		fs::create_dir(&root).expect("fixture root");
		fs::create_dir(&repository).expect("fixture repository");
		fs::set_permissions(&root, Permissions::from_mode(0o700)).expect("private fixture root");
		fs::set_permissions(&repository, Permissions::from_mode(0o700))
			.expect("private fixture repository");
		fs::create_dir(root.join("server")).expect("server directory");
		fs::set_permissions(root.join("server"), Permissions::from_mode(0o700))
			.expect("private server directory");

		write_private(&root.join("server/identity"), format!("{SERVER_ID}\n").as_bytes());

		Self { _temp: temp, root, repository }
	}

	fn config(&self, address: SocketAddr, repository: &str, pin: Option<&str>) -> String {
		let pin =
			pin.map(|pin| format!("expected_server_identity = \"{pin}\"\n")).unwrap_or_default();
		let uid = fs::metadata(&self.root).expect("root metadata").uid();

		format!(
			r#"version = 1
active_profile = "local"

[profiles.local]
kind = "local"
address = "{address}"
{pin}
[profiles.remote]
kind = "remote"
host = "192.0.2.1"
port = {}
expected_server_identity = "{SERVER_ID}"

[server_host.repositories.fixture]
host_path = "{repository}"

[postgres]
socket_directory = "{}"
expected_peer_uid = {uid}
port = 5432
database = "decodex"

[postgres.migration]
user = "decodex_migration"

[postgres.runtime]
user = "decodex_runtime"

[cache]
max_entries = 16
max_bytes = 65536
max_entry_bytes = 4096
"#,
			address.port(),
			self.root.join("missing-postgres-socket").display(),
		)
	}

	fn write_config(&self, body: &str) {
		write_private(&self.root.join("config.toml"), body.as_bytes());
	}

	fn run(&self, command: &str, extra: &[&str]) -> Output {
		let binary = env::var_os("DECODEX_TEST_CLI_BINARY")
			.expect("fixture runner supplies the built CLI binary");

		Command::new(binary)
			.args(["--root", self.root.to_str().expect("UTF-8 fixture root")])
			.args(["--output", "json"])
			.args(extra)
			.arg(command)
			.output()
			.expect("run real CLI process")
	}
}

fn loopback_reservation() -> (std::net::TcpListener, SocketAddr) {
	let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
		.expect("bind OS-selected loopback port");
	let address = listener.local_addr().expect("read OS-selected loopback port");

	(listener, address)
}

fn write_private(path: &Path, bytes: &[u8]) {
	let mut options = OpenOptions::new();

	options.create(true).truncate(true).write(true).mode(0o600);

	let mut file = options.open(path).expect("open private fixture file");

	file.write_all(bytes).expect("write private fixture file");
	file.sync_all().expect("sync private fixture file");
}

fn document(output: &Output) -> Value {
	serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
		panic!(
			"CLI output was not structured JSON: status={:?}, stdout={:?}, stderr={:?}",
			output.status.code(),
			String::from_utf8_lossy(&output.stdout),
			String::from_utf8_lossy(&output.stderr),
		)
	})
}

fn status<'a>(document: &'a Value, component: &str) -> &'a Value {
	document["report"]["checks"]
		.as_array()
		.expect("report checks")
		.iter()
		.find(|check| check["component"]["kind"] == component)
		.map(|check| &check["status"])
		.expect("typed component is present")
}

fn assert_redacted(fixture: &Fixture, output: &Output) {
	let combined = format!(
		"{}{}",
		String::from_utf8_lossy(&output.stdout),
		String::from_utf8_lossy(&output.stderr),
	);

	assert!(!combined.contains(SECRET_MARKER));
	assert!(!combined.contains(SERVER_PATH_MARKER));
	assert!(!combined.contains(
		fixture.root.parent().expect("fixture root has a parent").to_string_lossy().as_ref(),
	));
}

#[test]
#[ignore = "run through cargo make test-vnext-cli-diagnostics with the real CLI binary"]
fn malformed_config_and_missing_profile_process_failures_are_redacted() {
	let fixture = Fixture::new();
	let (_reservation, placeholder) = loopback_reservation();
	let missing = fixture.run("doctor", &[]);

	assert_eq!(missing.status.code(), Some(2));
	assert_eq!(document(&missing)["failure"], "configuration_missing");

	assert_redacted(&fixture, &missing);

	fs::set_permissions(&fixture.root, Permissions::from_mode(0o755))
		.expect("make fixture root unsafe");

	let unsafe_root = fixture.run("doctor", &[]);

	assert_eq!(unsafe_root.status.code(), Some(2));
	assert_eq!(document(&unsafe_root)["failure"], "unsafe_host_path");

	assert_redacted(&fixture, &unsafe_root);

	fs::set_permissions(&fixture.root, Permissions::from_mode(0o700))
		.expect("restore private fixture root");

	fixture.write_config(&format!("version = 1\npassword = \"{SECRET_MARKER}\"\n"));

	let malformed = fixture.run("doctor", &[]);

	assert_eq!(malformed.status.code(), Some(2));
	assert_eq!(document(&malformed)["failure"], "configuration_malformed");

	assert_redacted(&fixture, &malformed);

	fixture.write_config(&fixture.config(
		placeholder,
		fixture.repository.to_str().expect("UTF-8 fixture repository"),
		Some(SERVER_ID),
	));

	let missing = fixture.run("doctor", &["--profile", "missing"]);

	assert_eq!(missing.status.code(), Some(2));
	assert_eq!(document(&missing)["failure"], "profile_missing");

	assert_redacted(&fixture, &missing);
}

async fn closing_websocket_endpoint() -> (SocketAddr, JoinHandle<()>) {
	let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
		.await
		.expect("bind owned disconnect endpoint");
	let address = listener.local_addr().expect("owned disconnect address");
	let task = tokio::spawn(async move {
		let (stream, _) = time::timeout(FIXTURE_TIMEOUT, listener.accept())
			.await
			.expect("disconnect fixture accept timed out")
			.expect("accept disconnect fixture client");
		let mut socket = time::timeout(FIXTURE_TIMEOUT, tokio_tungstenite::accept_async(stream))
			.await
			.expect("disconnect fixture handshake timed out")
			.expect("complete disconnect fixture handshake");
		let hello = time::timeout(FIXTURE_TIMEOUT, socket.next())
			.await
			.expect("disconnect fixture hello timed out")
			.expect("client closed before hello")
			.expect("read disconnect fixture hello");

		assert!(matches!(hello, Message::Text(_)));

		time::timeout(FIXTURE_TIMEOUT, socket.close(None))
			.await
			.expect("disconnect fixture close timed out")
			.expect("close disconnect fixture session");
	});

	(address, task)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "run through cargo make test-vnext-cli-diagnostics with the real CLI binary"]
async fn real_cli_and_server_cover_status_doctor_identity_and_disconnected_states() {
	let fixture = Fixture::new();
	let (_reservation, placeholder) = loopback_reservation();

	fixture.write_config(&fixture.config(
		placeholder,
		fixture.repository.to_str().expect("UTF-8 fixture repository"),
		Some(SERVER_ID),
	));

	let bootstrap =
		ServiceComposition::bootstrap(DecodexRoot::new(&fixture.root).expect("safe fixture root"))
			.await;
	let bound = bootstrap
		.bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)), ServerConfig::default())
		.await
		.expect("bind isolated runtime");

	fixture.write_config(&fixture.config(
		bound.address(),
		fixture.repository.to_str().expect("UTF-8 fixture repository"),
		Some(SERVER_ID),
	));

	for command in ["status", "doctor"] {
		let output = fixture.run(command, &[]);
		let document = document(&output);

		assert_eq!(output.status.code(), Some(1), "{document}");
		assert_eq!(document["schema"], "decodex/cli-diagnostics/1");
		assert_eq!(document["command"], command);
		assert_eq!(document["outcome"], "report");
		assert_eq!(document["profile"], serde_json::json!({"kind": "local"}));
		assert_eq!(
			document["report"]["version"],
			serde_json::json!({
				"major": CURRENT_VERSION.major,
				"minor": CURRENT_VERSION.minor,
			}),
		);
		assert_eq!(
			status(&document, "database"),
			&serde_json::json!({"state": "unavailable", "issue": "database_unreachable"}),
		);
		assert_eq!(
			status(&document, "blob_integrity"),
			&serde_json::json!({"state": "unknown", "issue": "not_probed"}),
		);
		assert_eq!(
			status(&document, "credential_vault"),
			&serde_json::json!({"state": "unknown", "issue": "authentication"}),
		);
		assert_eq!(
			status(&document, "plugin_readiness"),
			&serde_json::json!({"state": "unknown", "issue": "plugin"}),
		);

		assert_redacted(&fixture, &output);
	}

	fixture.write_config(&fixture.config(
		bound.address(),
		fixture.repository.to_str().expect("UTF-8 fixture repository"),
		Some(WRONG_SERVER_ID),
	));

	let wrong_server = fixture.run("doctor", &[]);

	assert_eq!(wrong_server.status.code(), Some(2));
	assert_eq!(document(&wrong_server)["failure"], "server_identity_mismatch");

	assert_redacted(&fixture, &wrong_server);

	bound.shutdown().await.expect("shutdown isolated runtime");

	let (disconnect_address, disconnect_task) = closing_websocket_endpoint().await;

	fixture.write_config(&fixture.config(
		disconnect_address,
		fixture.repository.to_str().expect("UTF-8 fixture repository"),
		Some(SERVER_ID),
	));

	let disconnected = fixture.run("status", &[]);

	assert_eq!(disconnected.status.code(), Some(2));
	assert_eq!(document(&disconnected)["failure"], "protocol_disconnected");

	assert_redacted(&fixture, &disconnected);

	time::timeout(FIXTURE_TIMEOUT, disconnect_task)
		.await
		.expect("disconnect fixture task timed out")
		.expect("disconnect fixture task failed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "run through cargo make test-vnext-cli-diagnostics with the real CLI binary"]
async fn server_paths_are_not_reinterpreted_or_disclosed_by_the_client() {
	let fixture = Fixture::new();
	let (_reservation, placeholder) = loopback_reservation();
	let unsafe_path = format!("../{SERVER_PATH_MARKER}");

	fixture.write_config(&fixture.config(placeholder, &unsafe_path, Some(SERVER_ID)));

	let bootstrap =
		ServiceComposition::bootstrap(DecodexRoot::new(&fixture.root).expect("safe fixture root"))
			.await;
	let bound = bootstrap
		.bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)), ServerConfig::default())
		.await
		.expect("bind isolated runtime");

	fixture.write_config(&fixture.config(bound.address(), &unsafe_path, Some(SERVER_ID)));

	let output = fixture.run("doctor", &[]);
	let document = document(&output);

	assert_eq!(output.status.code(), Some(1), "{document}");
	assert_eq!(
		status(&document, "server_repositories"),
		&serde_json::json!({"state": "unavailable", "issue": "unsafe_host_path"}),
	);

	assert_redacted(&fixture, &output);

	bound.shutdown().await.expect("shutdown isolated runtime");
}
