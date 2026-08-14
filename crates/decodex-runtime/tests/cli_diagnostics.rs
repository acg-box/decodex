//! Real-process XY-1308 CLI/server diagnostic fixture matrix.
#![cfg(unix)]
#![allow(unused_crate_dependencies)]

use std::{
	env,
	fs::{self, OpenOptions, Permissions},
	io::Write as _,
	os::unix::fs::{MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _},
	path::{Path, PathBuf},
	process::{Command, Output},
};

use serde_json::Value;
use tempfile::TempDir;

use decodex_core::DecodexRoot;
use decodex_protocol::CURRENT_VERSION;
use decodex_runtime::{ServerConfig, ServiceComposition};

const SERVER_ID: &str = "018f0f9e-7b6e-4a31-8f4c-1d2e3f405162";
const WRONG_SERVER_ID: &str = "128f0f9e-7b6e-4a31-8f4c-1d2e3f405162";
const SECRET_MARKER: &str = "xy1308-secret-must-never-appear";

struct Fixture {
	_temp: TempDir,
	root: PathBuf,
}
impl Fixture {
	fn new() -> Self {
		let temp = TempDir::new().expect("fixture temp");
		let canonical = temp.path().canonicalize().expect("canonical fixture temp");
		let root = canonical.join(".decodex");

		fs::create_dir(&root).expect("fixture root");
		fs::set_permissions(&root, Permissions::from_mode(0o700)).expect("private fixture root");
		fs::create_dir(root.join("server")).expect("server directory");
		fs::set_permissions(root.join("server"), Permissions::from_mode(0o700))
			.expect("private server directory");

		write_private(&root.join("server/identity"), format!("{SERVER_ID}\n").as_bytes());

		Self { _temp: temp, root }
	}

	fn config(&self, pin: Option<&str>) -> String {
		let pin =
			pin.map(|pin| format!("expected_server_identity = \"{pin}\"\n")).unwrap_or_default();
		let uid = fs::metadata(&self.root).expect("root metadata").uid();

		format!(
			r#"version = 1
active_profile = "local"

[profiles.local]
kind = "local"
policy = "same_uid"
service_owner_uid = {uid}
{pin}
[profiles.remote]
kind = "remote"
host = "192.0.2.1"
port = 49152
expected_server_identity = "{SERVER_ID}"

[cache]
max_entries = 16
max_bytes = 65536
max_entry_bytes = 4096
"#,
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
	assert!(!combined.contains(
		fixture.root.parent().expect("fixture root has a parent").to_string_lossy().as_ref(),
	));
}

#[test]
#[ignore = "run through cargo make test-vnext-cli-diagnostics with the real CLI binary"]
fn malformed_config_and_missing_profile_process_failures_are_redacted() {
	let fixture = Fixture::new();
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

	fixture.write_config(&fixture.config(Some(SERVER_ID)));

	let missing = fixture.run("doctor", &["--profile", "missing"]);

	assert_eq!(missing.status.code(), Some(2));
	assert_eq!(document(&missing)["failure"], "profile_missing");

	assert_redacted(&fixture, &missing);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "run through cargo make test-vnext-cli-diagnostics with the real CLI binary"]
async fn real_cli_and_server_cover_status_doctor_identity_and_disconnected_states() {
	let fixture = Fixture::new();

	fixture.write_config(&fixture.config(Some(SERVER_ID)));

	let bootstrap =
		ServiceComposition::bootstrap(DecodexRoot::new(&fixture.root).expect("safe fixture root"))
			.await;
	let mut bound = bootstrap.bind(ServerConfig::default()).await.expect("bind isolated runtime");

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
		assert_eq!(status(&document, "product_store"), &serde_json::json!({"state": "ready"}),);
		assert_eq!(
			status(&document, "blob_integrity"),
			&serde_json::json!({"state": "unknown", "issue": "not_probed"}),
		);
		assert_eq!(status(&document, "credential_vault"), &serde_json::json!({"state": "ready"}),);
		assert_eq!(
			status(&document, "plugin_readiness"),
			&serde_json::json!({"state": "unknown", "issue": "plugin"}),
		);

		assert_redacted(&fixture, &output);
	}

	fixture.write_config(&fixture.config(Some(WRONG_SERVER_ID)));

	let wrong_server = fixture.run("doctor", &[]);

	assert_eq!(wrong_server.status.code(), Some(2));
	assert_eq!(document(&wrong_server)["failure"], "server_identity_mismatch");

	assert_redacted(&fixture, &wrong_server);

	bound.shutdown().await.expect("shutdown isolated runtime");

	fixture.write_config(&fixture.config(Some(SERVER_ID)));

	let disconnected = fixture.run("status", &[]);

	assert_eq!(disconnected.status.code(), Some(2));
	assert_eq!(document(&disconnected)["failure"], "protocol_disconnected");

	assert_redacted(&fixture, &disconnected);
}
