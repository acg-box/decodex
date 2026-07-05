use std::{ffi::OsString, fs};

use tempfile::TempDir;

use crate::github::{self, GhCommandDiscoveryTier};

#[test]
fn gh_command_resolution_prefers_path_candidate() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let gh_path = temp_dir.path().join("gh");

	fs::write(&gh_path, "").expect("fake gh should write");

	let resolution = github::gh_command_resolution_from_env(
		None,
		Some(OsString::from(temp_dir.path().as_os_str())),
		None,
	);

	assert_eq!(resolution.command_path(), gh_path.as_path());
	assert_eq!(resolution.resolved_path(), Some(gh_path.as_path()));
	assert_eq!(resolution.discovery_tier(), GhCommandDiscoveryTier::Path);
}
