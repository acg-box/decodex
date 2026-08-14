//! XY-1306 owned-root, containment, file-kind, symlink, and permission coverage.

#[path = "support/test_root.rs"] mod support;

use std::{fs, path::Path};

use getrandom as _;
#[cfg(unix)] use libc as _;
use regex as _;
use serde as _;
use serde_json as _;
use sha2 as _;
use tempfile::NamedTempFile;
use toml as _;

use decodex_core::{ConfigError, DecodexConfig, DecodexRoot, PathError};
use support::TestRoot;

#[test]
fn every_owned_path_is_derived_below_the_decodex_root() {
	let fixture = TestRoot::new();
	let root = fixture.paths.root().as_path();

	for path in [
		fixture.paths.config_file(),
		fixture.paths.logs_dir(),
		fixture.paths.blobs_dir(),
		fixture.paths.cache_dir(),
		fixture.paths.server_dir(),
		fixture.paths.server_identity_file(),
		fixture.paths.credential_vault_file(),
		fixture.paths.product_database_file(),
	] {
		assert!(path.starts_with(root));
		assert!(!path.components().any(|component| component.as_os_str() == ".codex"));
	}
}

#[test]
fn root_and_layout_debug_never_expose_path_text() {
	let root = DecodexRoot::new("/private/xy1306-sensitive-root/.decodex")
		.expect("lexically valid marked root");

	assert!(!format!("{root:?}").contains("xy1306-sensitive-root"));
	assert!(!format!("{:?}", root.paths()).contains("xy1306-sensitive-root"));
}

#[test]
fn unsafe_ambiguous_and_codex_owned_roots_fail_before_writes() {
	assert_eq!(DecodexRoot::new("relative/.decodex"), Err(PathError::UnsafeRoot));
	assert_eq!(DecodexRoot::new(Path::new("/")), Err(PathError::UnsafeRoot));
	assert_eq!(DecodexRoot::new(Path::new("/tmp/parent/../.decodex")), Err(PathError::UnsafeRoot),);
	assert_eq!(DecodexRoot::new("/tmp/nul\0root"), Err(PathError::UnsafeRoot));
	assert_eq!(
		DecodexRoot::new(format!("/tmp/{}", "x".repeat(4 * 1_024))),
		Err(PathError::UnsafeRoot),
	);

	for root in ["/tmp/.codex/decodex", "/tmp/.Codex/decodex", "/tmp/.CODEX/decodex"] {
		assert_eq!(DecodexRoot::new(Path::new(root)), Err(PathError::CodexOwnedRoot));
	}
}

#[test]
fn accepted_roots_are_stored_in_one_lexically_normalized_form() {
	let root = DecodexRoot::new("/tmp/xy1306-root//nested/./.decodex")
		.expect("absolute root without parent traversal");

	assert_eq!(root.as_path(), Path::new("/tmp/xy1306-root/nested/.decodex"));
}

#[test]
fn rejecting_a_codex_owned_root_leaves_codex_home_untouched() {
	let home = tempfile::tempdir().expect("temporary home");
	let codex_home = home.path().join(".codex");

	fs::create_dir(&codex_home).expect("Codex-owned directory fixture");

	assert_eq!(DecodexRoot::new(codex_home.join("decodex")), Err(PathError::CodexOwnedRoot),);
	assert_eq!(fs::read_dir(&codex_home).expect("read Codex home").count(), 0);
}

#[test]
fn normal_layout_creation_never_writes_to_the_neighboring_codex_home() {
	let home = tempfile::tempdir().expect("temporary home");
	let canonical_home = home.path().canonicalize().expect("canonical temporary home");
	let codex_home = canonical_home.join(".codex");
	let sentinel = codex_home.join("codex-owned-sentinel");

	fs::create_dir(&codex_home).expect("Codex-owned directory fixture");
	fs::write(&sentinel, b"Codex-owned").expect("Codex-owned sentinel fixture");

	let paths = DecodexRoot::from_home(&canonical_home).expect("safe sibling Decodex root").paths();

	paths.ensure_layout().expect("Decodex-owned layout");

	assert_eq!(fs::read(&sentinel).expect("unchanged Codex sentinel"), b"Codex-owned");
	assert_eq!(fs::read_dir(&codex_home).expect("read Codex home").count(), 1);
	assert!(paths.root().as_path().starts_with(&canonical_home));
	assert!(!paths.root().as_path().starts_with(&codex_home));
}

#[test]
fn fixed_layout_creation_is_idempotent() {
	let fixture = TestRoot::new();

	fixture.paths.ensure_layout().expect("first layout creation");
	fixture.paths.ensure_layout().expect("second layout verification");

	for directory in [
		fixture.paths.root().as_path().to_path_buf(),
		fixture.paths.logs_dir(),
		fixture.paths.blobs_dir(),
		fixture.paths.cache_dir(),
		fixture.paths.server_dir(),
	] {
		assert!(directory.is_dir());
	}
}

#[test]
fn unexpected_config_file_kind_fails_closed() {
	let fixture = TestRoot::new();

	fixture.paths.ensure_layout().expect("private layout");

	fs::create_dir(fixture.paths.config_file()).expect("directory in file position");

	assert!(matches!(
		DecodexConfig::load(&fixture.paths),
		Err(ConfigError::Path(PathError::UnexpectedFileKind)),
	));
}

#[cfg(unix)]
#[test]
fn symlinked_owned_directory_fails_closed() {
	let fixture = TestRoot::new();

	fixture.paths.ensure_layout().expect("private layout");

	let outside = tempfile::tempdir().expect("outside target");

	fs::remove_dir(fixture.paths.cache_dir()).expect("remove empty cache directory");
	std::os::unix::fs::symlink(outside.path(), fixture.paths.cache_dir())
		.expect("cache symlink fixture");

	assert_eq!(fixture.paths.ensure_layout(), Err(PathError::Symlink));
}

#[cfg(unix)]
#[test]
fn symlinked_root_ancestor_cannot_redirect_writes_into_codex_home() {
	let home = tempfile::tempdir().expect("temporary home");
	let codex_home = home.path().join(".codex");
	let alias = home.path().join("state-alias");

	fs::create_dir(&codex_home).expect("Codex home fixture");
	std::os::unix::fs::symlink(&codex_home, &alias).expect("ancestor symlink fixture");

	let root = DecodexRoot::new(alias.join("decodex-state")).expect("lexically separate root");

	assert_eq!(root.paths().ensure_layout(), Err(PathError::Symlink));
	assert_eq!(fs::read_dir(&codex_home).expect("read Codex home").count(), 0);
}

#[cfg(unix)]
#[test]
fn insecure_root_permissions_fail_closed() {
	let fixture = TestRoot::new();

	fixture.paths.ensure_layout().expect("private layout");

	support::set_mode(fixture.paths.root().as_path(), 0o755);

	assert_eq!(fixture.paths.ensure_layout(), Err(PathError::InsecurePermissions));
}

#[cfg(unix)]
#[test]
fn a_symlinked_config_file_is_never_followed() {
	let fixture = TestRoot::new();

	fixture.paths.ensure_layout().expect("private layout");

	let outside = NamedTempFile::new().expect("outside config");

	std::os::unix::fs::symlink(outside.path(), fixture.paths.config_file())
		.expect("config symlink fixture");

	assert!(matches!(
		DecodexConfig::load(&fixture.paths),
		Err(ConfigError::Path(PathError::Symlink)),
	));
}
