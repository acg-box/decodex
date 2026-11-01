//! The caller can exit while an independent worker publishes a durable result.
#![allow(unused_crate_dependencies)]

use std::{
	fs,
	process::Command,
	thread,
	time::{Duration, Instant},
};

use decodex_pro_bridge::{Config, ProBridge};

#[test]
fn installed_helper_survives_the_original_build_path_and_uninstalls_without_losing_results() {
	let home = tempfile::tempdir().unwrap();
	let original = home.path().join("development-build");
	fs::copy(env!("CARGO_BIN_EXE_decodex-pro-bridge"), &original).unwrap();
	let root = home.path().join("component");
	let registry = home.path().join("registry");
	decodex_pro_bridge::install::install(&root, &registry, &original).unwrap();
	fs::remove_file(original).unwrap();
	let managed = root.join("bin/decodex-pro-bridge");
	let output = Command::new(managed).arg("--help").output().unwrap();
	assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
	let bridge = ProBridge::new(Config::default(), &root).unwrap();
	bridge
		.prepare(decodex_pro_bridge::Request {
			id: "retained".into(),
			prompt: "Retain this input".into(),
			previous: None,
			project: None,
		})
		.unwrap();
	decodex_pro_bridge::install::uninstall(&root, &registry).unwrap();
	assert!(bridge.status("retained").is_ok());
	assert!(!root.join("bin/decodex-pro-bridge").exists());
}

#[test]
fn detached_worker_records_startup_failure_after_the_calling_process_exits() {
	let directory = tempfile::tempdir().unwrap();
	let prompt = directory.path().join("question.txt");
	fs::write(&prompt, "An isolated question. No browser is available in this fixture.").unwrap();
	let result = Command::new(env!("CARGO_BIN_EXE_decodex-pro-bridge"))
		.arg("--state-dir")
		.arg(directory.path())
		.args(["start", "--id", "background"])
		.arg("--prompt-file")
		.arg(&prompt)
		.output()
		.unwrap();
	assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
	let bridge = ProBridge::new(Config::default(), directory.path()).unwrap();
	let deadline = Instant::now() + Duration::from_secs(5);
	loop {
		if let Some(error) = bridge.status("background").unwrap().last_error {
			assert!(error.contains("not connected"));
			break;
		}
		assert!(Instant::now() < deadline, "Independent worker did not publish its failure.");
		thread::sleep(Duration::from_millis(10));
	}
}
