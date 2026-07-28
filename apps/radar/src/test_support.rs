use std::{
	os::unix::fs::PermissionsExt as _,
	path::PathBuf,
	sync::{Mutex, MutexGuard, OnceLock},
};

pub(crate) struct TestEnvLockGuard {
	_lock: MutexGuard<'static, ()>,
}

pub(crate) fn lock_test_env() -> TestEnvLockGuard {
	TestEnvLockGuard {
		_lock: test_env_mutex().lock().expect("test env mutex should not be poisoned"),
	}
}

pub(crate) fn private_tempdir() -> tempfile::TempDir {
	let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
	let workspace = manifest
		.parent()
		.and_then(std::path::Path::parent)
		.expect("Radar manifest must be inside the workspace");
	let parent = workspace.join("target/radar-private-tests");

	std::fs::create_dir_all(&parent).expect("private test root should be created");
	std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o700))
		.expect("private test root should be owner-only");
	tempfile::tempdir_in(parent).expect("private temporary directory should be created")
}

fn test_env_mutex() -> &'static Mutex<()> {
	static TEST_ENV_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

	TEST_ENV_MUTEX.get_or_init(|| Mutex::new(()))
}
