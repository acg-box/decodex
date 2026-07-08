mod github_landing_status;
mod privacy_accounts;
mod review_autonomy;
mod secrets_validation;
mod service_paths;

use std::{
	env,
	ffi::OsString,
	fs,
	path::{Path, PathBuf},
	sync::{Mutex, MutexGuard, OnceLock},
};

struct TestEnvVarGuard {
	key: String,
	previous: Option<OsString>,
}
impl TestEnvVarGuard {
	fn lock() -> MutexGuard<'static, ()> {
		static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

		ENV_LOCK
			.get_or_init(|| Mutex::new(()))
			.lock()
			.expect("env var mutex should not be poisoned")
	}

	fn set(key: &str, value: &str) -> Self {
		let _guard = Self::lock();
		let previous = env::var_os(key);

		unsafe { env::set_var(key, value) };

		Self { key: key.to_owned(), previous }
	}
}

impl Drop for TestEnvVarGuard {
	fn drop(&mut self) {
		match self.previous.take() {
			Some(previous) => unsafe { env::set_var(&self.key, previous) },
			None => unsafe { env::remove_var(&self.key) },
		}
	}
}

fn write_config_file(dir: &Path, body: &str) -> PathBuf {
	let config_path = dir.join("project.toml");
	let body = body_with_explicit_repo_root(body);

	fs::write(&config_path, body).expect("config should write");

	config_path
}

fn body_with_explicit_repo_root(body: &str) -> String {
	if body.contains("repo_root") {
		return body.to_owned();
	}
	if body.contains("[paths]") {
		return body.replacen("[paths]", "[paths]\nrepo_root = \".\"", 1);
	}

	format!("{body}\n\n[paths]\nrepo_root = \".\"\n")
}
