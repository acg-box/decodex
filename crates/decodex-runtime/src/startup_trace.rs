//! Opt-in, value-free startup phase timings for local performance diagnosis.
use std::{sync::LazyLock, time::Instant};

static ENABLED: LazyLock<bool> =
	LazyLock::new(|| std::env::var_os("DECODEX_TRACE_STARTUP").is_some());

pub(crate) struct Phase {
	name: &'static str,
	started: Option<Instant>,
}
impl Phase {
	pub(crate) fn new(name: &'static str) -> Self {
		Self { name, started: ENABLED.then(Instant::now) }
	}
}
impl Drop for Phase {
	fn drop(&mut self) {
		if let Some(started) = self.started {
			eprintln!(
				"decodex_startup phase={} elapsed_ms={}",
				self.name,
				started.elapsed().as_millis()
			);
		}
	}
}
