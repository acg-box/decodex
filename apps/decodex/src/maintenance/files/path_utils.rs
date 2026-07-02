use std::{
	fs::{self, Metadata, OpenOptions},
	path::{Path, PathBuf},
	time::{Duration, SystemTime},
};

use time::OffsetDateTime;

use crate::prelude::{Result, eyre};

pub(super) fn copy_truncate(path: &Path, rotated_path: &Path) -> Result<()> {
	if let Some(parent) = rotated_path.parent() {
		fs::create_dir_all(parent)?;
	}

	fs::copy(path, rotated_path)?;
	OpenOptions::new().write(true).truncate(true).open(path)?;

	Ok(())
}

pub(super) fn rotated_path(path: &Path, generated_at: OffsetDateTime) -> Result<PathBuf> {
	let parent = path.parent().ok_or_else(|| {
		eyre::eyre!("Maintenance target `{}` has no parent directory.", path.display())
	})?;
	let file_name = path.file_name().and_then(|name| name.to_str()).ok_or_else(|| {
		eyre::eyre!("Maintenance target `{}` has no UTF-8 file name.", path.display())
	})?;
	let Some((prefix, suffix)) = file_name.rsplit_once('.') else {
		return Ok(parent.join(format!("{file_name}.{}", generated_at.unix_timestamp())));
	};
	let candidate = parent.join(format!("{prefix}.{}.{suffix}", generated_at.unix_timestamp()));

	next_available_path(candidate)
}

pub(super) fn file_is_older_than(
	metadata: &Metadata,
	system_now: SystemTime,
	retention: Duration,
) -> bool {
	metadata
		.modified()
		.ok()
		.and_then(|modified| system_now.duration_since(modified).ok())
		.is_some_and(|age| age > retention)
}

pub(super) fn is_rotated_log_file(path: &Path) -> bool {
	path.file_stem()
		.and_then(|stem| stem.to_str())
		.and_then(|stem| stem.rsplit_once('.').map(|(_, timestamp)| timestamp))
		.is_some_and(|timestamp| timestamp.parse::<i64>().is_ok())
}

fn next_available_path(path: PathBuf) -> Result<PathBuf> {
	if !path.exists() {
		return Ok(path);
	}

	let parent = path.parent().ok_or_else(|| {
		eyre::eyre!("Maintenance target `{}` has no parent directory.", path.display())
	})?;
	let file_name = path.file_name().and_then(|name| name.to_str()).ok_or_else(|| {
		eyre::eyre!("Maintenance target `{}` has no UTF-8 file name.", path.display())
	})?;

	for index in 1..=999 {
		let candidate = parent.join(format!("{file_name}.{index}"));

		if !candidate.exists() {
			return Ok(candidate);
		}
	}

	eyre::bail!("Could not allocate a unique maintenance rotation path for `{}`.", path.display());
}
