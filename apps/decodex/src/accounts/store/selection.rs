use std::{fs, path::Path};

use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	accounts::{
		auth_json::{self, AuthDotJson},
		random_names,
		record::AccountPoolRecord,
		store::AccountStore,
		types::{AccountListResponse, AccountUseResponse},
	},
	prelude::{Result, eyre},
};

impl AccountStore {
	pub(in crate::accounts) fn select(&self, selector: &str) -> Result<AccountListResponse> {
		let selector = selector.trim();

		if selector.is_empty() {
			eyre::bail!("Codex account selector cannot be empty.");
		}

		let records = self.load_records()?;

		if !records.iter().any(|record| record.matches_account_selector(selector)) {
			eyre::bail!("No Decodex account matches selector `{selector}`.");
		}

		self.write_fixed_account_selector(Some(selector))?;

		self.response_from_records(&records)
	}

	pub(in crate::accounts) fn clear_selection(&self) -> Result<AccountListResponse> {
		let records = self.load_records()?;

		self.write_fixed_account_selector(None)?;

		self.response_from_records(&records)
	}

	pub(in crate::accounts) fn logout(&self, selector: &str) -> Result<AccountListResponse> {
		let selector = selector.trim();

		if selector.is_empty() {
			eyre::bail!("Codex account selector cannot be empty.");
		}

		let mut records = self.load_records()?;
		let selector_matched_fixed =
			self.fixed_account_selector()?.as_deref().is_some_and(|fixed| {
				fixed == selector
					|| records.iter().any(|record| {
						record.matches_account_selector(selector)
							&& record.matches_account_selector(fixed)
					})
			});
		let original_len = records.len();

		records.retain(|record| !record.matches_account_selector(selector));

		if records.len() == original_len {
			eyre::bail!("No Decodex account matches selector `{selector}`.");
		}

		self.save_records(&records)?;

		if selector_matched_fixed {
			self.write_fixed_account_selector(None)?;
		}

		self.response_from_records(&records)
	}

	pub(in crate::accounts) fn reroll_name(
		&self,
		selector: &str,
		offset: Option<i64>,
	) -> Result<AccountListResponse> {
		let selector = selector.trim();

		if selector.is_empty() {
			eyre::bail!("Codex account selector cannot be empty.");
		}

		let records = self.load_records()?;
		let record = records
			.iter()
			.find(|record| record.matches_account_selector(selector))
			.ok_or_else(|| eyre::eyre!("No Decodex account matches selector `{selector}`."))?;
		let key = record.random_name_key();
		let offsets = self.account_name_offsets()?;
		let current = offsets.get(&key).copied().unwrap_or_default();
		let next = offset.map_or_else(
			|| random_names::normalize_random_name_offset(current + 1),
			random_names::normalize_random_name_offset,
		);

		self.write_account_name_offset(&key, next)?;

		self.response_from_records(&records)
	}

	pub(in crate::accounts) fn import_auth_json(
		&self,
		auth_json_path: &Path,
	) -> Result<AccountListResponse> {
		let input = fs::read_to_string(auth_json_path).map_err(|error| {
			eyre::eyre!("Failed to read Codex auth JSON `{}`: {error}", auth_json_path.display())
		})?;
		let auth = serde_json::from_str::<AuthDotJson>(&input).map_err(|error| {
			eyre::eyre!("Codex auth JSON `{}` is invalid: {error}", auth_json_path.display())
		})?;
		let mut record = AccountPoolRecord::from_auth(auth)?;
		let mut records = self.load_records()?;

		if record.last_refresh.is_none() {
			record.last_refresh = Some(now_rfc3339()?);
		}

		let replace_index = records.iter().position(|candidate| {
			record.account_id().is_some() && candidate.account_id() == record.account_id()
				|| record.email().is_some() && candidate.email() == record.email()
		});

		if let Some(index) = replace_index {
			records[index] = record;
		} else {
			records.push(record);
		}

		self.save_records(&records)?;

		self.response_from_records(&records)
	}

	pub(in crate::accounts) fn use_for_codex(
		&self,
		selector: &str,
		auth_json_path: Option<&Path>,
	) -> Result<AccountUseResponse> {
		let selector = selector.trim();

		if selector.is_empty() {
			eyre::bail!("Codex account selector cannot be empty.");
		}

		let records = self.load_records()?;
		let record = records
			.iter()
			.find(|record| record.matches_account_selector(selector))
			.ok_or_else(|| eyre::eyre!("No Decodex account matches selector `{selector}`."))?;

		if record.disabled {
			eyre::bail!("Decodex account `{selector}` is disabled and cannot be used by Codex.");
		}
		if record.auth_failure().is_some() {
			eyre::bail!(
				"Decodex account `{selector}` is auth_failed and must be re-logged before Codex can use it."
			);
		}

		record.validate_importable()?;

		let target_path = auth_json_path.unwrap_or(&self.codex_auth_path);

		auth_json::write_auth_json_atomically(target_path, &record.auth_dot_json()?)?;

		Ok(AccountUseResponse {
			codex_auth_path: target_path.display().to_string(),
			account: record.identity_summary(),
		})
	}
}

fn now_rfc3339() -> Result<String> {
	Ok(OffsetDateTime::now_utc().format(&Rfc3339)?)
}
