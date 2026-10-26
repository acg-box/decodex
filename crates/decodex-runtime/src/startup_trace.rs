//! Opt-in, value-free startup phase timings for local performance diagnosis.
use std::{env, fs::OpenOptions, io::Write as _, path::PathBuf, sync::LazyLock, time::Instant};

static OUTPUT: LazyLock<Option<PathBuf>> =
	LazyLock::new(|| env::var_os("DECODEX_STARTUP_TRACE_PATH").map(Into::into));
static ENABLED: LazyLock<bool> =
	LazyLock::new(|| env::var_os("DECODEX_TRACE_STARTUP").is_some() || OUTPUT.is_some());

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

pub(crate) fn archive_pending(count: usize) {
	if *ENABLED {
		write(&format!("decodex_startup phase=archive_visibility pending={count}"));
	}
}

fn write(line: &str) {
	if let Some(path) = OUTPUT.as_ref() {
		if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
			let _ = writeln!(file, "{line}");
		}
	} else {
		eprintln!("{line}");
	}
}
