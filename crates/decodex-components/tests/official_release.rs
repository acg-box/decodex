//! Opt-in acceptance against the real signed release, using an isolated component store.
use decodex_codex as _;
use decodex_components::{ComponentStore, RuntimeLock};
use reqwest as _;
use semver as _;
use serde as _;
use serde_json as _;
use sha2 as _;
use std::{
	fs,
	path::Path,
	process::{Command, Output},
};

fn prepare(store: &Path, offline: bool) -> Output {
	let mut command = Command::new(env!("CARGO_BIN_EXE_decodex-components"));
	command
		.args([
			"prepare",
			concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/official-release.json"),
			"--store",
		])
		.arg(store);
	if offline {
		for variable in ["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"] {
			command.env(variable, "http://127.0.0.1:9");
		}
		command.env("NO_PROXY", "").env("no_proxy", "");
	}
	command.output().expect("component command")
}

fn success(output: Output) {
	assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn failed_pin_preserves_the_existing_lock() {
	let temporary = tempfile::tempdir().expect("lock directory");
	let lock = temporary.path().join("codex-runtime.lock.json");
	fs::write(&lock, "existing pin").expect("existing lock");
	let output = Command::new(env!("CARGO_BIN_EXE_decodex-components"))
		.arg("pin")
		.arg(&lock)
		.arg(concat!(env!("CARGO_MANIFEST_DIR"), "/../../codex-runtime.lock.json"))
		.env("DECODEX_CODEX_RELEASE", "not-a-version")
		.output()
		.expect("pin command");
	assert!(!output.status.success());
	assert_eq!(fs::read_to_string(lock).expect("unchanged lock"), "existing pin");
}

#[test]
#[ignore = "requires macOS and downloads the official signed Codex release twice"]
fn official_download_retry_offline_reuse_and_corrupt_schema_repair() {
	let temporary = tempfile::tempdir().expect("isolated store");
	let root = temporary.path().join("components");
	let store = ComponentStore::new(root.clone());
	let lock = RuntimeLock::parse(include_bytes!("fixtures/official-release.json"))
		.expect("official fixture");
	let paths = store.paths(&lock).expect("runtime paths");
	assert!(!prepare(&root, true).status.success());
	assert!(!paths.root.exists(), "failed first download must not publish a cache");
	success(prepare(&root, false));
	assert!(store.installed(&lock).expect("verified package").is_some());
	success(prepare(&root, true));
	let previous = paths.root.with_file_name("previous-version");
	fs::create_dir(&previous).expect("previous version");
	fs::write(previous.join("keep"), "older runtime").expect("previous runtime");
	let schema = paths.schemas.join("ClientRequest.json");
	fs::write(&schema, "corrupt schema").expect("simulate cache damage");
	assert!(store.installed(&lock).is_err());
	assert!(!prepare(&root, true).status.success());
	assert_eq!(
		fs::read_to_string(&schema).expect("failed repair preserves cache"),
		"corrupt schema"
	);
	success(prepare(&root, false));
	assert!(store.installed(&lock).expect("repaired package").is_some());
	success(prepare(&root, true));
	assert_eq!(
		fs::read_to_string(previous.join("keep")).expect("previous version preserved"),
		"older runtime"
	);
	assert!(
		fs::read_dir(paths.root.parent().expect("store parent"))
			.expect("no partial downloads")
			.all(|entry| !entry
				.expect("entry")
				.file_name()
				.to_string_lossy()
				.starts_with(".download-"))
	);
}
