use std::path::{Path, PathBuf};

use time::OffsetDateTime;

use crate::prelude::{Result, eyre};

pub(crate) fn usage_history_path(accounts_path: &Path) -> Result<PathBuf> {
	let parent = accounts_path.parent().ok_or_else(|| {
		eyre::eyre!(
			"Decodex accounts path `{}` must have a parent directory.",
			accounts_path.display()
		)
	})?;

	Ok(parent.join("account-usage-history.jsonl"))
}

pub(crate) fn usage_record_date(unix_epoch: i64) -> Option<String> {
	OffsetDateTime::from_unix_timestamp(unix_epoch)
		.ok()
		.map(|timestamp| timestamp.date().to_string())
}
