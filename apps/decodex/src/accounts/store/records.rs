use std::{fs, io::ErrorKind, path::Path, process};

use crate::{
	accounts::{
		auth_json::AuthDotJson,
		file_security,
		identity::AccountIdentity,
		random_names,
		record::{AccountPoolLine, AccountPoolRecord},
		store::AccountStore,
		types::{AccountControlSummary, AccountListResponse},
	},
	prelude::{Result, eyre},
};

impl AccountStore {
	pub(in crate::accounts) fn save_records(&self, records: &[AccountPoolRecord]) -> Result<()> {
		let parent = self.accounts_path.parent().ok_or_else(|| {
			eyre::eyre!(
				"Decodex accounts path `{}` must have a parent directory.",
				self.accounts_path.display()
			)
		})?;
		let file_name =
			self.accounts_path.file_name().and_then(|name| name.to_str()).ok_or_else(|| {
				eyre::eyre!("Decodex accounts path must end in a valid file name.")
			})?;
		let temp_path = parent.join(format!(".{file_name}.tmp-{}", process::id()));
		let mut body = String::new();

		for record in records {
			body.push_str(&serde_json::to_string(record)?);
			body.push('\n');
		}

		fs::create_dir_all(parent)?;
		fs::write(&temp_path, body)?;
		file_security::secure_account_file(&temp_path)?;
		fs::rename(temp_path, &self.accounts_path)?;
		file_security::secure_account_file(&self.accounts_path)?;

		Ok(())
	}

	pub(in crate::accounts::store) fn load_records(&self) -> Result<Vec<AccountPoolRecord>> {
		let input = match fs::read_to_string(&self.accounts_path) {
			Ok(input) => input,
			Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
			Err(error) => {
				eyre::bail!(
					"Failed to read Decodex accounts `{}`: {error}",
					self.accounts_path.display()
				);
			},
		};

		parse_account_records(&input, &self.accounts_path)
	}

	pub(in crate::accounts::store) fn response_from_records(
		&self,
		records: &[AccountPoolRecord],
	) -> Result<AccountListResponse> {
		let selector = self.fixed_account_selector()?;
		let codex_auth = self.codex_auth_identity().unwrap_or_default();
		let name_offsets = self.account_name_offsets()?;
		let control = AccountControlSummary {
			mode: if selector.is_some() { String::from("fixed") } else { String::from("balanced") },
			account_selector: selector.clone(),
		};
		let mut accounts = records
			.iter()
			.map(|record| record.summary(selector.as_deref(), codex_auth.as_ref(), &name_offsets))
			.collect::<Vec<_>>();

		random_names::assign_unique_random_names(&mut accounts);

		Ok(AccountListResponse {
			accounts_path: self.accounts_path.display().to_string(),
			global_config_path: self.global_config_path.display().to_string(),
			codex_auth_path: self.codex_auth_path.display().to_string(),
			codex_auth: codex_auth.as_ref().map(AccountIdentity::summary),
			control,
			accounts,
			usage_estimate: None,
			usage_probe_error: None,
		})
	}

	pub(in crate::accounts::store) fn codex_auth_identity(
		&self,
	) -> Result<Option<AccountIdentity>> {
		let input = match fs::read_to_string(&self.codex_auth_path) {
			Ok(input) => input,
			Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
			Err(error) => {
				eyre::bail!(
					"Failed to read Codex auth JSON `{}`: {error}",
					self.codex_auth_path.display()
				);
			},
		};
		let auth = serde_json::from_str::<AuthDotJson>(&input).map_err(|error| {
			eyre::eyre!("Codex auth JSON `{}` is invalid: {error}", self.codex_auth_path.display())
		})?;
		let record = AccountPoolRecord::from_auth(auth)?;

		Ok(Some(record.identity()))
	}
}

fn parse_account_records(input: &str, path: &Path) -> Result<Vec<AccountPoolRecord>> {
	let mut records = Vec::new();

	for (line_index, line) in input.lines().enumerate() {
		let line_number = line_index + 1;
		let trimmed = line.trim();

		if trimmed.is_empty() || trimmed.starts_with('#') {
			continue;
		}

		let parsed = serde_json::from_str::<AccountPoolLine>(trimmed).map_err(|error| {
			eyre::eyre!(
				"Decodex accounts `{}` line {line_number} is not a valid auth JSONL entry: {error}",
				path.display()
			)
		})?;

		records.push(parsed.into_record()?);
	}

	Ok(records)
}
