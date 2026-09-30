//! CLI regression for release backfill temporary-file ownership.

#![allow(unused_crate_dependencies)]

use std::{fs, process::Command};

#[test]
fn failed_backfill_refresh_removes_its_temporary_directory() {
	let cwd = tempfile::tempdir().unwrap();

	for relative in ["automations/radar/radar.toml", "apps/radar/src/lib.rs"] {
		let path = cwd.path().join(relative);

		fs::create_dir_all(path.parent().unwrap()).unwrap();
		fs::write(path, "").unwrap();
	}

	let temp_root = cwd.path().join("temporary");

	fs::create_dir(&temp_root).unwrap();

	let output = Command::new(env!("CARGO_BIN_EXE_radar"))
		.current_dir(cwd.path())
		.env("TMPDIR", &temp_root)
		.env("TMP", &temp_root)
		.env("TEMP", &temp_root)
		.env_remove("RADAR_BACKFILL_TEST_MISSING_TOKEN")
		.args([
			"backfill-release-range",
			"--dry-run",
			"--refresh-release-delta-first",
			"--token-env",
			"RADAR_BACKFILL_TEST_MISSING_TOKEN",
		])
		.output()
		.expect("Radar CLI");

	assert!(!output.status.success());

	let stderr = String::from_utf8_lossy(&output.stderr);

	assert!(stderr.contains("RADAR_BACKFILL_TEST_MISSING_TOKEN is missing or empty"), "{stderr}");
	assert_eq!(fs::read_dir(&temp_root).unwrap().count(), 0, "failed refresh left temporary files");
}
