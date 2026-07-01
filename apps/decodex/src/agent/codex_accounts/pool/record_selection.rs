use color_eyre::Report;
use time::OffsetDateTime;

use crate::{
	agent::codex_accounts::{
		CodexAccountAuthFailure, CodexAccountLogin, pool::CodexAccountPool,
		record::AccountPoolRecord, refresh::RefreshStatus, selection,
	},
	prelude::eyre,
};

impl CodexAccountPool {
	pub(in crate::agent::codex_accounts::pool) fn select_from_records(
		&self,
		records: &mut [AccountPoolRecord],
	) -> crate::prelude::Result<CodexAccountLogin> {
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

	fn select_fixed_from_records(
		&self,
		records: &mut [AccountPoolRecord],
		selector: &str,
	) -> crate::prelude::Result<CodexAccountLogin> {
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

	fn account_candidate_from_record(
		&self,
		record: &mut AccountPoolRecord,
		line_number: usize,
		now: i64,
		records_changed: &mut bool,
	) -> crate::prelude::Result<std::result::Result<CodexAccountLogin, String>> {
		if record.disabled {
			return Ok(Err(format!("line {line_number} disabled")));
		}

		if let Some(auth_failure) = record.auth_failure() {
			return Ok(Err(format!("line {line_number} auth failed: {auth_failure}")));
		}

		if record.cooldown_until_unix_epoch.is_some_and(|cooldown| cooldown > now) {
			return Ok(Err(format!("line {line_number} cooling down")));
		}
		if record.account_id().is_none() {
			return Ok(Err(format!("line {line_number} missing account id")));
		}
		if record.access_token().is_none() {
			return Ok(Err(format!("line {line_number} missing access token")));
		}

		let refresh_status = match self.proactive_refresh_record(record, now) {
			Ok(status) => {
				if status == RefreshStatus::Succeeded {
					*records_changed = true;
				}

				status.as_str()
			},
			Err(error) if error.auth_failed => {
				*records_changed = true;

				return Ok(Err(format!("{} auth failed: {}", record.display_name(), error.source)));
			},
			Err(error) if error.requires_skip => {
				return Ok(Err(format!(
					"{} proactive refresh failed: {}",
					record.display_name(),
					error.source
				)));
			},
			Err(_error) => RefreshStatus::Failed.as_str(),
		};

		match self.probe_record_usage(record) {
			Ok(usage) => Ok(Ok(record.login_from_usage(usage, refresh_status)?)),
			Err(error) if error.unauthorized && record.refresh_token().is_some() => {
				if let Err(refresh_error) = self.refresh_record(record) {
					if let Some(auth_failure) =
						refresh_error.downcast_ref::<CodexAccountAuthFailure>()
					{
						*records_changed = true;

						return Ok(Err(format!(
							"{} auth failed: {auth_failure}",
							record.display_name()
						)));
					}

					return Err(refresh_error);
				}

				*records_changed = true;

				let usage = self.probe_record_usage(record).map_err(|retry_error| {
					eyre::eyre!(
						"Codex account `{}` refreshed but usage probe still failed: {retry_error}",
						record.display_name()
					)
				})?;

				Ok(Ok(record.login_from_usage(usage, "succeeded")?))
			},
			Err(error) => Ok(Err(format!("{} usage probe failed: {error}", record.display_name()))),
		}
	}

	pub(in crate::agent::codex_accounts::pool) fn fixed_record_index(
		&self,
		records: &[AccountPoolRecord],
		selector: &str,
	) -> crate::prelude::Result<usize> {
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
