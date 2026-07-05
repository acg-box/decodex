use std::{env, ffi::OsString};

pub(crate) struct TestEnvVarGuard {
	pub(crate) key: String,
	pub(crate) previous: Option<OsString>,
}
impl TestEnvVarGuard {
	pub(crate) fn set(key: impl Into<String>, value: &str) -> Self {
		let key = key.into();
		let previous = env::var_os(&key);

		unsafe { env::set_var(&key, value) };

		Self { key, previous }
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
