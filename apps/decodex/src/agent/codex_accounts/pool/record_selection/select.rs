use color_eyre::Report;
use time::OffsetDateTime;

use crate::{
	agent::codex_accounts::{
		CodexAccountLogin, pool::CodexAccountPool, record::AccountPoolRecord, selection,
	},
	prelude::{Result, eyre},
};

impl CodexAccountPool {
	pub(in crate::agent::codex_accounts::pool) fn select_from_records(
		&self,
		records: &mut [AccountPoolRecord],
	) -> Result<CodexAccountLogin> {
		if let Some(selector) = self.fixed_account.as_deref() {
			return self.select_fixed_from_records(records, selector);
		}

		let now = OffsetDateTime::now_utc().unix_timestamp();
		let mut candidates = Vec::new();
		let mut skipped = Vec::new();
		let mut records_changed = false;

		for (index, record) in records.iter_mut().enumerate() {
			match self.account_candidate_from_record(
				record,
				index + 1,
				now,
				&mut records_changed,
			)? {
				Ok(candidate) => candidates.push(candidate),
				Err(reason) => skipped.push(reason),
			}
		}

		if records_changed {
			self.save_records(records)?;
		}
		if candidates.is_empty() {
			if let Some(auth_failure) =
				records.iter().find_map(AccountPoolRecord::auth_failed_error)
			{
				return Err(Report::new(auth_failure));
			}

			eyre::bail!(
				"No usable Codex account was available from `{}`. Skipped entries: {}",
				self.path.display(),
				if skipped.is_empty() { String::from("none") } else { skipped.join("; ") }
			);
		}

		candidates.sort_by(selection::compare_account_candidates);

		let mut selected = candidates.remove(0);

		selected.mark_selected(now);

		if let Some(record) =
			records.iter_mut().find(|record| record.account_id() == Some(selected.account_id()))
		{
			record.last_selected_at_unix_epoch = Some(now);
			records_changed = true;
		}

		let account_summaries = selection::account_summaries(&selected, &candidates);
		let selected = selected.with_account_summaries(account_summaries);

		if records_changed {
			self.save_records(records)?;
		}

		self.remember_selected_account(&selected.account_id)?;

		Ok(selected)
	}
}
