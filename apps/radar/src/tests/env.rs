use std::{env, ffi::OsString};

use crate::test_support::{self, TestEnvLockGuard};

pub(in crate::tests) struct TestEnvVars {
	_lock: TestEnvLockGuard,
	previous: Vec<(String, Option<OsString>)>,
}
impl TestEnvVars {
	pub(in crate::tests) fn set(vars: &[(&str, Option<&str>)]) -> Self {
		let lock = test_support::lock_test_env();
		let previous =
			vars.iter().map(|(key, _)| ((*key).to_owned(), env::var_os(key))).collect::<Vec<_>>();

		for (key, value) in vars {
			match value {
				Some(value) => unsafe { env::set_var(key, value) },
				None => unsafe { env::remove_var(key) },
			}
		}

		Self { _lock: lock, previous }
	}
}

impl Drop for TestEnvVars {
	fn drop(&mut self) {
		for (key, previous) in self.previous.drain(..).rev() {
			match previous {
				Some(previous) => unsafe { env::set_var(key, previous) },
				None => unsafe { env::remove_var(key) },
			}
		}
	}
}
