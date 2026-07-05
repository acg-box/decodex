use std::{ffi::OsString, fs};

use tempfile::TempDir;

use crate::github::{self, GhCommandDiscoveryTier};

#[test]
fn gh_command_resolution_uses_configured_path_as_authority() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let gh_path = temp_dir.path().join("configured-gh");

	fs::write(&gh_path, "").expect("fake configured gh should write");

	let resolution =
		github::gh_command_resolution_from_env(Some(&gh_path), Some(OsString::new()), None);

	assert_eq!(resolution.command_path(), gh_path.as_path());
	assert_eq!(resolution.configured_path(), Some(gh_path.as_path()));
	assert_eq!(resolution.resolved_path(), Some(gh_path.as_path()));
	assert_eq!(resolution.discovery_tier(), GhCommandDiscoveryTier::Configured);
}
