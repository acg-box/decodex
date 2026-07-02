use std::{
	collections::BTreeMap,
	fs,
	io::ErrorKind,
	path::{Path, PathBuf},
	process,
};

use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use toml::{Table, Value};

use crate::{
	accounts::{
		auth_json::{self, AuthDotJson},
		file_security,
		identity::AccountIdentity,
		random_names,
		record::{AccountPoolLine, AccountPoolRecord},
		types::{AccountControlSummary, AccountListResponse, AccountUseResponse},
	},
	prelude::{Result, eyre},
	runtime,
};

pub(crate) struct AccountStore {
	pub(super) accounts_path: PathBuf,
	pub(super) global_config_path: PathBuf,
	codex_auth_path: PathBuf,
}
impl AccountStore {
	pub(crate) fn global() -> Result<Self> {
		Ok(Self {
			accounts_path: runtime::accounts_path()?,
			global_config_path: runtime::global_config_path()?,
			codex_auth_path: auth_json::default_codex_auth_json_path()?,
		})
	}

	#[cfg(test)]
	pub(super) fn new(accounts_path: PathBuf, global_config_path: PathBuf) -> Self {
		let codex_auth_path = accounts_path
			.parent()
			.map(|parent| parent.join("auth.json"))
			.unwrap_or_else(|| PathBuf::from("auth.json"));

		Self { accounts_path, global_config_path, codex_auth_path }
	}

	#[cfg(test)]
	pub(super) fn new_with_codex_auth_path(
		accounts_path: PathBuf,
		global_config_path: PathBuf,
		codex_auth_path: PathBuf,
	) -> Self {
		Self { accounts_path, global_config_path, codex_auth_path }
	}

	pub(super) fn list(&self) -> Result<AccountListResponse> {
		let records = self.load_records()?;

		self.response_from_records(&records)
	}

	pub(super) fn list_with_cached_usage(
		&self,
		force_refresh: bool,
	) -> Result<AccountListResponse> {
		let mut response = self.list()?;

		response.hydrate_usage_from_path(&self.accounts_path, force_refresh);

		Ok(response)
	}

	pub(super) fn select(&self, selector: &str) -> Result<AccountListResponse> {
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

	pub(super) fn clear_selection(&self) -> Result<AccountListResponse> {
		let records = self.load_records()?;

		self.write_fixed_account_selector(None)?;

		self.response_from_records(&records)
	}

	pub(super) fn logout(&self, selector: &str) -> Result<AccountListResponse> {
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

	pub(super) fn reroll_name(
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

	pub(super) fn import_auth_json(&self, auth_json_path: &Path) -> Result<AccountListResponse> {
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

	pub(super) fn use_for_codex(
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

	pub(super) fn save_records(&self, records: &[AccountPoolRecord]) -> Result<()> {
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

	fn load_records(&self) -> Result<Vec<AccountPoolRecord>> {
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

	fn response_from_records(&self, records: &[AccountPoolRecord]) -> Result<AccountListResponse> {
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

	fn codex_auth_identity(&self) -> Result<Option<AccountIdentity>> {
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

	fn fixed_account_selector(&self) -> Result<Option<String>> {
		let document = self.load_global_config_document()?;
		let selector = document
			.get("codex")
			.and_then(Value::as_table)
			.and_then(|codex| codex.get("accounts"))
			.and_then(Value::as_table)
			.and_then(|accounts| accounts.get("fixed_account"))
			.and_then(Value::as_str)
			.map(str::trim)
			.filter(|value| !value.is_empty())
			.map(str::to_owned);

		Ok(selector)
	}

	fn account_name_offsets(&self) -> Result<BTreeMap<String, i64>> {
		let document = self.load_global_config_document()?;
		let Some(offsets) = document
			.get("codex")
			.and_then(Value::as_table)
			.and_then(|codex| codex.get("account_names"))
			.and_then(Value::as_table)
			.and_then(|account_names| account_names.get("offsets"))
			.and_then(Value::as_table)
		else {
			return Ok(BTreeMap::new());
		};

		Ok(offsets
			.iter()
			.filter_map(|(key, value)| {
				let key = key.trim();

				(!key.is_empty()).then_some((
					key.to_owned(),
					random_names::normalize_random_name_offset(
						value.as_integer().unwrap_or_default(),
					),
				))
			})
			.collect())
	}

	fn write_account_name_offset(&self, key: &str, offset: i64) -> Result<()> {
		let key = key.trim();

		if key.is_empty() {
			eyre::bail!("Codex account name key cannot be empty.");
		}

		let mut document = self.load_global_config_document()?;
		let offsets = ensure_toml_table(
			ensure_toml_table(ensure_toml_table(&mut document, "codex")?, "account_names")?,
			"offsets",
		)?;

		offsets.insert(
			key.to_owned(),
			Value::Integer(random_names::normalize_random_name_offset(offset)),
		);

		self.write_global_config_document(&document)
	}

	fn write_fixed_account_selector(&self, selector: Option<&str>) -> Result<()> {
		let mut document = self.load_global_config_document()?;

		match selector.map(str::trim).filter(|value| !value.is_empty()) {
			Some(selector) => {
				let accounts =
					ensure_toml_table(ensure_toml_table(&mut document, "codex")?, "accounts")?;

				accounts.insert(String::from("fixed_account"), selector.to_owned().into());
			},
			None => {
				if let Some(codex) = document.get_mut("codex").and_then(Value::as_table_mut)
					&& let Some(accounts) = codex.get_mut("accounts").and_then(Value::as_table_mut)
				{
					accounts.remove("fixed_account");
				}
			},
		}

		self.write_global_config_document(&document)
	}

	fn load_global_config_document(&self) -> Result<Table> {
		let input = match fs::read_to_string(&self.global_config_path) {
			Ok(input) => input,
			Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Table::new()),
			Err(error) => {
				eyre::bail!(
					"Failed to read Decodex global config `{}`: {error}",
					self.global_config_path.display()
				);
			},
		};

		if input.trim().is_empty() { Ok(Table::new()) } else { Ok(toml::from_str(&input)?) }
	}

	fn write_global_config_document(&self, document: &Table) -> Result<()> {
		let parent = self.global_config_path.parent().ok_or_else(|| {
			eyre::eyre!(
				"Decodex global config `{}` must have a parent directory.",
				self.global_config_path.display()
			)
		})?;
		let file_name =
			self.global_config_path.file_name().and_then(|name| name.to_str()).ok_or_else(
				|| eyre::eyre!("Decodex global config path must end in a valid file name."),
			)?;
		let temp_path = parent.join(format!(".{file_name}.tmp-{}", process::id()));
		let output = toml::to_string_pretty(&document)?;

		fs::create_dir_all(parent)?;
		fs::write(&temp_path, output)?;
		file_security::secure_account_file(&temp_path)?;
		fs::rename(temp_path, &self.global_config_path)?;
		file_security::secure_account_file(&self.global_config_path)?;

		Ok(())
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

fn ensure_toml_table<'a>(parent: &'a mut Table, key: &str) -> Result<&'a mut Table> {
	if !parent.contains_key(key) {
		parent.insert(String::from(key), Value::Table(Table::new()));
	}

	parent
		.get_mut(key)
		.and_then(Value::as_table_mut)
		.ok_or_else(|| eyre::eyre!("`{key}` in Decodex global config must be a table."))
}

fn now_rfc3339() -> Result<String> {
	Ok(OffsetDateTime::now_utc().format(&Rfc3339)?)
}
