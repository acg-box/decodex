//! Opt-in qualification against an existing fixed-commit native artifact, without installation.
use decodex_codex as _;
use decodex_components::{ComponentStore, RuntimeLock};
use reqwest as _;
use semver as _;
use serde as _;
use serde_json as _;
use sha2 as _;
use std::{fs, path::PathBuf};

#[test]
#[ignore = "requires a qualified macOS runtime in DECODEX_TEST_DEVELOPMENT_RUNTIME"]
fn fixed_commit_import_offline_repair_and_wrong_commit_rejection() {
	let artifact = PathBuf::from(
		std::env::var_os("DECODEX_TEST_DEVELOPMENT_RUNTIME").expect("qualified runtime"),
	);
	let temporary = tempfile::tempdir().expect("isolated store");
	let store = ComponentStore::new(temporary.path().into());
	let lock = RuntimeLock::embedded().expect("fixed development pin");
	store.import_development(&lock, &artifact, |_| {}).expect("verify and import");
	let installed = store.ensure(&lock, |_| {}).expect("offline cached component");
	assert_eq!(
		fs::read(&installed.executable).expect("cached image"),
		fs::read(artifact.join("CodexCLI.app/Contents/MacOS/codex")).expect("qualified image")
	);
	let previous = installed.root.with_file_name("another-version");
	fs::create_dir(&previous).expect("older version");
	fs::write(previous.join("keep"), "preserve").expect("older data");
	fs::write(&installed.executable, "damaged binary").expect("damage cache");
	assert!(store.installed(&lock).is_err());
	store.ensure(&lock, |_| {}).expect("repair from verified local archive");
	fs::write(installed.schemas.join("ClientRequest.json"), "damaged schema")
		.expect("damage schema");
	store.ensure(&lock, |_| {}).expect("repair schema from exact artifact");
	for relative in ["bin/codex-code-mode-host", "codex-path/rg"] {
		let helper = installed.root.join(relative);
		let expected = fs::read(artifact.join(relative)).expect("qualified helper");
		fs::write(&helper, "damaged helper").expect("damage isolated helper");
		assert!(store.installed(&lock).is_err(), "reject damaged {relative}");
		store.ensure(&lock, |_| {}).expect("repair helper from exact artifact");
		assert_eq!(fs::read(&helper).expect("repaired helper"), expected);
	}
	assert!(store.installed(&lock).expect("fully verified").is_some());
	assert_eq!(fs::read_to_string(previous.join("keep")).expect("older data"), "preserve");

	assert!(
		store.import_development(&lock, &temporary.path().join("missing-runtime"), |_| {}).is_err()
	);
	assert_eq!(
		store.paths(&lock).expect("failed import preserves selection").executable,
		installed.executable
	);
	assert!(store.installed(&lock).expect("previous artifact stays valid").is_some());

	let mut wrong = lock.clone();
	wrong.upstream_commit = "f".repeat(40);
	wrong.source.as_mut().expect("source").url =
		format!("https://codeload.github.com/openai/codex/tar.gz/{}", wrong.upstream_commit);
	assert!(
		store
			.import_development(&wrong, &artifact, |_| {})
			.expect_err("wrong commit")
			.contains("pinned commit")
	);
	assert!(store.installed(&wrong).expect("wrong commit was not published").is_none());
	assert!(store.installed(&lock).expect("original pin still valid").is_some());
}
