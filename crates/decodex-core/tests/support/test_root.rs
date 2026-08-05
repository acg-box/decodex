#![allow(dead_code)]

#[cfg(unix)] use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
use std::{
	fs::{self, OpenOptions, Permissions},
	io::Write as _,
	path::{Path, PathBuf},
};

use tempfile::TempDir;

use decodex_core::{DecodexPaths, DecodexRoot};

pub const SERVER_ID: &str = "018f0f9e-7b6e-4a31-8f4c-1d2e3f405162";

pub struct TestRoot {
	_temp: TempDir,
	pub paths: DecodexPaths,
}
impl TestRoot {
	pub fn new() -> Self {
		let temp = tempfile::tempdir().expect("temporary home");
		let canonical_home = temp.path().canonicalize().expect("canonical temporary home");
		let root = DecodexRoot::from_home(canonical_home).expect("safe temporary Decodex root");
		let paths = root.paths();

		Self { _temp: temp, paths }
	}
}

pub fn valid_config() -> String {
	#[cfg(unix)]
	// SAFETY: `geteuid` has no arguments or failure return.
	let service_owner_uid = unsafe { libc::geteuid() };
	#[cfg(not(unix))]
	let service_owner_uid = 501;

	format!(
		r#"version = 1
active_profile = "local"

[profiles.local]
kind = "local"
policy = "same_uid"
service_owner_uid = {service_owner_uid}

[profiles.remote]
kind = "remote"
host = "server.example.test"
port = 49152
expected_server_identity = "{SERVER_ID}"

[postgres]
socket_directory = "/var/run/postgresql"
expected_peer_uid = 70
port = 5432
database = "decodex"

[postgres.runtime]
user = "decodex_runtime"
credential_env_var = "DECODEX_POSTGRES_RUNTIME_PASSWORD"

[cache]
max_entries = 128
max_bytes = 1048576
max_entry_bytes = 65536
"#,
	)
}

pub fn write_private(path: &Path, bytes: &[u8]) {
	let mut options = OpenOptions::new();

	options.write(true).create(true).truncate(true);
	#[cfg(unix)]
	options.mode(0o600);

	let mut file = options.open(path).expect("private fixture file");

	file.write_all(bytes).expect("write private fixture");
}

pub fn write_private_config(root: &TestRoot, bytes: &[u8]) {
	root.paths.ensure_layout().expect("private layout");

	write_private(&root.paths.config_file(), bytes);
}

pub fn private_file_names(directory: &Path) -> Vec<PathBuf> {
	let mut names = fs::read_dir(directory)
		.expect("read private directory")
		.map(|entry| entry.expect("directory entry").path())
		.collect::<Vec<_>>();

	names.sort();

	names
}

#[cfg(unix)]
pub fn set_mode(path: &Path, mode: u32) {
	fs::set_permissions(path, Permissions::from_mode(mode)).expect("set fixture mode");
}
