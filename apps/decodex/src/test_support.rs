use std::{
	env,
	ffi::{OsStr, OsString},
	process::Command,
	sync::{Mutex, MutexGuard, OnceLock},
};

pub(crate) struct TestEnvVarGuard {
	_lock: MutexGuard<'static, ()>,
	previous: Vec<(String, Option<OsString>)>,
}
impl TestEnvVarGuard {
	pub(crate) fn set(key: impl Into<String>, value: &str) -> Self {
		Self::set_many([(key.into(), value.to_owned())])
	}

	pub(crate) fn set_many<K, V, I>(vars: I) -> Self
	where
		K: Into<String>,
		V: AsRef<OsStr>,
		I: IntoIterator<Item = (K, V)>,
	{
		let lock = test_env_mutex().lock().expect("test env mutex should not be poisoned");
		let mut previous = Vec::new();

		for (key, value) in vars {
			let key = key.into();

			previous.push((key.clone(), env::var_os(&key)));

			unsafe { env::set_var(&key, value) };
		}

		Self { _lock: lock, previous }
	}

	pub(crate) fn lock() -> TestEnvLockGuard {
		TestEnvLockGuard {
			_lock: test_env_mutex().lock().expect("test env mutex should not be poisoned"),
		}
	}
}

impl Drop for TestEnvVarGuard {
	fn drop(&mut self) {
		while let Some((key, previous)) = self.previous.pop() {
			match previous {
				Some(previous) => unsafe { env::set_var(&key, previous) },
				None => unsafe { env::remove_var(&key) },
			}
		}
	}
}

pub(crate) struct TestEnvLockGuard {
	_lock: MutexGuard<'static, ()>,
}

pub(crate) fn hermetic_git_command() -> Command {
	let mut command = Command::new("git");

	command
		.env("GIT_CONFIG_GLOBAL", "/dev/null")
		.env("GIT_CONFIG_SYSTEM", "/dev/null")
		.env("GIT_TERMINAL_PROMPT", "0")
		.env("GCM_INTERACTIVE", "never")
		.args([
			"-c",
			"core.hooksPath=/dev/null",
			"-c",
			"commit.gpgsign=false",
			"-c",
			"tag.gpgsign=false",
			"-c",
			"init.defaultBranch=main",
		]);

	command
}

fn test_env_mutex() -> &'static Mutex<()> {
	static TEST_ENV_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

	TEST_ENV_MUTEX.get_or_init(|| Mutex::new(()))
}
