use std::{
	env,
	path::{Path, PathBuf},
	thread,
	time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{prelude::Result, runtime, state::StateStore};

pub(crate) fn current_timestamp() -> String {
	OffsetDateTime::now_utc().format(&Rfc3339).expect("timestamp formatting should succeed")
}

pub(crate) fn build_run_id(issue_identifier: &str, attempt_number: i64) -> Result<String> {
	let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();

	Ok(format!("{}-attempt-{attempt_number}-{timestamp}", issue_identifier.to_lowercase()))
}

pub(crate) fn resolve_config_path(
	explicit_path: Option<&Path>,
	state_store: &StateStore,
) -> Result<Option<PathBuf>> {
	if let Some(path) = explicit_path {
		return Ok(Some(path.to_path_buf()));
	}

	runtime::registered_config_path_for_cwd(state_store, &env::current_dir()?)
}

pub(crate) fn sleep_until_next_tick(poll_interval: Duration, tick_started_at: Instant) {
	let elapsed = tick_started_at.elapsed();

	if elapsed < poll_interval {
		thread::sleep(poll_interval - elapsed);
	}
}
