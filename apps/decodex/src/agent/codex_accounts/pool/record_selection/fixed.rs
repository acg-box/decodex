use color_eyre::Report;
use time::OffsetDateTime;

use crate::{
	agent::codex_accounts::{
		CodexAccountLogin, pool::CodexAccountPool, record::AccountPoolRecord, selection,
	},
	prelude::{Result, eyre},
};

impl CodexAccountPool {
	pub(in crate::agent::codex_accounts::pool) fn select_fixed_from_records(
		&self,
		records: &mut [AccountPoolRecord],
		selector: &str,
	) -> Result<CodexAccountLogin> {
		let record_index = self.fixed_record_index(records, selector)?;
		let now = OffsetDateTime::now_utc().unix_timestamp();
		let mut records_changed = false;

		if let Some(auth_failure) = records[record_index].auth_failed_error() {
			return Err(Report::new(auth_failure));
		}

		let mut selected = match self.account_candidate_from_record(
			&mut records[record_index],
			record_index + 1,
			now,
			&mut records_changed,
		)? {
			Ok(candidate) => candidate,
			Err(reason) => {
				eyre::bail!(
					"Configured Codex fixed account `{selector}` from `{}` is not usable: {reason}",
					self.path.display()
				);
			},
		};

		selected.mark_selected(now);

		records[record_index].last_selected_at_unix_epoch = Some(now);
		records_changed = true;

		let account_summaries = selection::account_summaries(&selected, &[]);
		let selected = selected.with_account_summaries(account_summaries);

		if records_changed {
			self.save_records(records)?;
		}

		self.remember_selected_account(&selected.account_id)?;

		Ok(selected)
	}

	pub(in crate::agent::codex_accounts::pool) fn fixed_record_index(
		&self,
		records: &[AccountPoolRecord],
		selector: &str,
	) -> Result<usize> {
		let matches = records
			.iter()
			.enumerate()
			.filter_map(|(index, record)| {
				record.matches_account_selector(selector).then_some(index)
			})
			.collect::<Vec<_>>();

		match matches.as_slice() {
			[] => eyre::bail!(
				"Configured Codex fixed account `{selector}` does not match any account in `{}`.",
				self.path.display()
			),
			[index] => Ok(*index),
			_ => eyre::bail!(
				"Configured Codex fixed account `{selector}` matched multiple accounts in `{}`.",
				self.path.display()
			),
		}
	}
}
