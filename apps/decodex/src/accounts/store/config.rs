use std::{collections::BTreeMap, fs, io::ErrorKind, process};

use toml::{Table, Value};

use crate::{
	accounts::{file_security, random_names, store::AccountStore},
	prelude::{Result, eyre},
};

impl AccountStore {
	pub(in crate::accounts::store) fn fixed_account_selector(&self) -> Result<Option<String>> {
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

	pub(in crate::accounts::store) fn account_name_offsets(&self) -> Result<BTreeMap<String, i64>> {
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

	pub(in crate::accounts::store) fn write_account_name_offset(
		&self,
		key: &str,
		offset: i64,
	) -> Result<()> {
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

	pub(in crate::accounts::store) fn write_fixed_account_selector(
		&self,
		selector: Option<&str>,
	) -> Result<()> {
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

	pub(in crate::accounts::store) fn load_global_config_document(&self) -> Result<Table> {
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

	pub(in crate::accounts::store) fn write_global_config_document(
		&self,
		document: &Table,
	) -> Result<()> {
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

fn ensure_toml_table<'a>(parent: &'a mut Table, key: &str) -> Result<&'a mut Table> {
	if !parent.contains_key(key) {
		parent.insert(String::from(key), Value::Table(Table::new()));
	}

	parent
		.get_mut(key)
		.and_then(Value::as_table_mut)
		.ok_or_else(|| eyre::eyre!("`{key}` in Decodex global config must be a table."))
}
