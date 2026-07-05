use std::{ffi::OsString, fs};

use tempfile::TempDir;

use crate::github::{self, GhCommandDiscoveryTier};

#[test]
fn gh_command_resolution_falls_back_to_home_local_bin() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let bin_dir = temp_dir.path().join(".local/bin");
	let gh_path = bin_dir.join("gh");

	fs::create_dir_all(&bin_dir).expect("fake home bin should exist");
	fs::write(&gh_path, "").expect("fake gh should write");

	let resolution = github::gh_command_resolution_from_env(
		None,
		Some(OsString::new()),
		Some(OsString::from(temp_dir.path().as_os_str())),
	);

	assert_eq!(resolution.command_path(), gh_path.as_path());
	assert_eq!(resolution.resolved_path(), Some(gh_path.as_path()));
	assert_eq!(resolution.discovery_tier(), GhCommandDiscoveryTier::UserBin);
}
