use std::{fs, io::ErrorKind, path::Path, process};

use time::OffsetDateTime;

use crate::{
	accounts::{
		file_security,
		types::AccountSummary,
		usage_history::{self, AccountUsageHistoryRecord, USAGE_ESTIMATE_WINDOW_DAYS},
	},
	prelude::{Result, eyre},
};

#[derive(Default)]
pub(crate) struct AccountUsageHistory {
	records: Vec<AccountUsageHistoryRecord>,
}
impl AccountUsageHistory {
	pub(crate) fn read(path: &Path) -> Result<Self> {
		let input = match fs::read_to_string(path) {
			Ok(input) => input,
			Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Self::default()),
			Err(error) => {
				eyre::bail!("Failed to read account usage history `{}`: {error}", path.display());
			},
		};

		Ok(Self { records: parse_usage_history_records(&input, path)? })
	}

	pub(crate) fn merge_current_records(
		&mut self,
		current_records: impl Iterator<Item = AccountUsageHistoryRecord>,
	) {
		let now = OffsetDateTime::now_utc().unix_timestamp();
		let current_records = current_records.collect::<Vec<_>>();

		self.records.retain(|record| {
			record.is_recent(now)
				&& !current_records.iter().any(|current| current.same_daily_slot(record))
		});
		self.records.extend(current_records);
		self.records.sort_by(|left, right| {
			left.date
				.cmp(&right.date)
				.then_with(|| left.account_fingerprint.cmp(&right.account_fingerprint))
		});
	}

	pub(crate) fn write(&self, path: &Path) -> Result<()> {
		let parent = path.parent().ok_or_else(|| {
			eyre::eyre!("Account usage history path `{}` must have a parent.", path.display())
		})?;
		let file_name = path.file_name().and_then(|name| name.to_str()).ok_or_else(|| {
			eyre::eyre!("Account usage history path must end in a valid file name.")
		})?;
		let temp_path = parent.join(format!(".{file_name}.tmp-{}", process::id()));
		let mut body = String::new();

		for record in &self.records {
			body.push_str(&serde_json::to_string(record)?);
			body.push('\n');
		}

		fs::create_dir_all(parent)?;
		fs::write(&temp_path, body)?;
		file_security::secure_account_file(&temp_path)?;
		fs::rename(temp_path, path)?;
		file_security::secure_account_file(path)?;

		Ok(())
	}

	pub(crate) fn apply_to_accounts(&self, accounts: &mut [AccountSummary]) {
		let now = OffsetDateTime::now_utc().unix_timestamp();

		for account in accounts {
			let matching_records = self
				.records
				.iter()
				.filter(|record| record.matches_account(account))
				.collect::<Vec<_>>();

			if let Some(latest) =
				matching_records.iter().max_by_key(|record| record.checked_at_unix_epoch)
			{
				if account.seven_day_used_percent.is_none() {
					account.seven_day_used_percent = Some(latest.used_percent);
					account.capacity_multiplier =
						usage_history::normalized_account_capacity_multiplier(
							latest.capacity_multiplier,
						);
					account.seven_day_daily_average_percent =
						Some(latest.used_percent as f64 / USAGE_ESTIMATE_WINDOW_DAYS as f64);
				}

				latest.apply_missing_usage_windows(account, now);
			}

			account.usage_records =
				matching_records.iter().map(|record| record.daily_summary()).collect();
		}
	}
}

fn parse_usage_history_records(input: &str, path: &Path) -> Result<Vec<AccountUsageHistoryRecord>> {
	let mut records = Vec::new();

	for (line_index, line) in input.lines().enumerate() {
		let line_number = line_index + 1;
		let trimmed = line.trim();

		if trimmed.is_empty() || trimmed.starts_with('#') {
			continue;
		}

		let record =
			serde_json::from_str::<AccountUsageHistoryRecord>(trimmed).map_err(|error| {
				eyre::eyre!(
					"Decodex account usage history `{}` line {line_number} is invalid: {error}",
					path.display()
				)
			})?;

		records.push(record);
	}

	Ok(records)
}
