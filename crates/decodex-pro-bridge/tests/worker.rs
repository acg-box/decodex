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
