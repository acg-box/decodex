use std::{
	fs::{self, OpenOptions},
	io::Write as _,
	path::Path,
	process,
	sync::atomic::{AtomicU64, Ordering},
};

use crate::{
	prelude::{Result, eyre},
	state::RUN_ACTIVITY_MARKER_FILE,
};

static RUN_ACTIVITY_MARKER_WRITE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(crate) fn write_run_activity_marker_body_atomic(marker_path: &Path, body: &str) -> Result<()> {
	let parent = marker_path.parent().ok_or_else(|| {
		eyre::eyre!("activity marker path `{}` has no parent directory", marker_path.display())
	})?;
	let sequence = RUN_ACTIVITY_MARKER_WRITE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
	let temp_path =
		parent.join(format!(".{RUN_ACTIVITY_MARKER_FILE}.{}.{}.tmp", process::id(), sequence,));
	let result = (|| -> Result<()> {
		let mut temp_file = OpenOptions::new().write(true).create_new(true).open(&temp_path)?;

		temp_file.write_all(body.as_bytes())?;
		temp_file.flush()?;

		drop(temp_file);

		fs::rename(&temp_path, marker_path)?;

		Ok(())
	})();

	if result.is_err() {
		let _ = fs::remove_file(&temp_path);
	}

	result
}
