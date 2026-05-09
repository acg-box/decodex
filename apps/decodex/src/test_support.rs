use std::{
	env,
	ffi::OsString,
	sync::{Mutex, MutexGuard, OnceLock},
};

pub(crate) struct TestEnvVarGuard {
	_lock: MutexGuard<'static, ()>,
	key: String,
	previous: Option<OsString>,
}
impl TestEnvVarGuard {
	pub(crate) fn set(key: impl Into<String>, value: &str) -> Self {
		let lock = test_env_mutex().lock().expect("test env mutex should not be poisoned");
		let key = key.into();
		let previous = env::var_os(&key);

		unsafe { env::set_var(&key, value) };

		Self { _lock: lock, key, previous }
	}

	pub(crate) fn lock() -> TestEnvLockGuard {
		TestEnvLockGuard {
			_lock: test_env_mutex().lock().expect("test env mutex should not be poisoned"),
		}
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

pub(crate) struct TestEnvLockGuard {
	_lock: MutexGuard<'static, ()>,
}

fn test_env_mutex() -> &'static Mutex<()> {
	static TEST_ENV_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

	TEST_ENV_MUTEX.get_or_init(|| Mutex::new(()))
}
