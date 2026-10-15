//! Opt-in, value-free startup phase timings for local performance diagnosis.
use std::{io::Write as _, sync::LazyLock, time::Instant};

static OUTPUT: LazyLock<Option<std::path::PathBuf>> =
	LazyLock::new(|| std::env::var_os("DECODEX_STARTUP_TRACE_PATH").map(Into::into));
static ENABLED: LazyLock<bool> =
	LazyLock::new(|| std::env::var_os("DECODEX_TRACE_STARTUP").is_some() || OUTPUT.is_some());

fn write(line: &str) {
	if let Some(path) = OUTPUT.as_ref() {
		if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
			let _ = writeln!(file, "{line}");
		}
	} else {
		eprintln!("{line}");
	}
}

pub(crate) fn archive_pending(count: usize) {
	if *ENABLED {
		write(&format!("decodex_startup phase=archive_visibility pending={count}"));
	}
}

pub(crate) struct Phase {
	name: &'static str,
	started: Option<Instant>,
}
impl Phase {
	pub(crate) fn new(name: &'static str) -> Self {
		if *ENABLED {
			write(&format!("decodex_startup phase={name} stage=start"));
		}
		Self { name, started: ENABLED.then(Instant::now) }
	}
}
impl Drop for Phase {
	fn drop(&mut self) {
		if let Some(started) = self.started {
			write(&format!(
				"decodex_startup phase={} elapsed_ms={}",
				self.name,
				started.elapsed().as_millis()
			));
		}
	}
}
