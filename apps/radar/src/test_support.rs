use std::sync::{Mutex, MutexGuard, OnceLock};

pub(crate) struct TestEnvLockGuard {
	_lock: MutexGuard<'static, ()>,
}

pub(crate) fn lock_test_env() -> TestEnvLockGuard {
	TestEnvLockGuard {
		_lock: test_env_mutex().lock().expect("test env mutex should not be poisoned"),
	}
}

fn test_env_mutex() -> &'static Mutex<()> {
	static TEST_ENV_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

	TEST_ENV_MUTEX.get_or_init(|| Mutex::new(()))
}
