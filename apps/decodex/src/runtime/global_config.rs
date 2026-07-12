//! Global Decodex operator config accessors.

#[cfg(test)] use std::process;
use std::{fs, io::ErrorKind};

use toml::Value;

use crate::{
	prelude::{Result, eyre},
	runtime,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TrackerCredentialConfig {
	pub(crate) credential_ref: String,
	pub(crate) provider: String,
	pub(crate) api_key_env_var: String,
}

/// Read host-level tracker credential references. Project contracts cannot supply these.
pub(crate) fn tracker_credential_catalog() -> Result<Vec<TrackerCredentialConfig>> {
	let config_path = runtime::global_config_path()?;
	let input = match fs::read_to_string(&config_path) {
		Ok(input) => input,
		Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
		Err(error) =>
			eyre::bail!("Failed to read Decodex global config `{}`: {error}", config_path.display()),
	};
	let document = toml::from_str::<toml::Table>(&input)?;
	let Some(credentials) = document
		.get("tracker")
		.and_then(Value::as_table)
		.and_then(|tracker| tracker.get("credentials"))
		.and_then(Value::as_array)
	else {
		return Ok(Vec::new());
	};
	let mut catalog = Vec::with_capacity(credentials.len());
	for credential in credentials {
		let table = credential
			.as_table()
			.ok_or_else(|| eyre::eyre!("Each tracker credential must be a TOML table."))?;
		let required = |field: &str| -> Result<String> {
			table
				.get(field)
				.and_then(Value::as_str)
				.map(str::trim)
				.filter(|value| !value.is_empty())
				.map(str::to_owned)
				.ok_or_else(|| eyre::eyre!("Tracker credential `{field}` is required."))
		};
		let entry = TrackerCredentialConfig {
			credential_ref: required("ref")?,
			provider: required("provider")?,
			api_key_env_var: required("api_key_env_var")?,
		};
		if entry.provider != "linear" {
			eyre::bail!("Unsupported tracker credential provider `{}`.", entry.provider);
		}
		if catalog.iter().any(|existing: &TrackerCredentialConfig| {
			existing.credential_ref == entry.credential_ref
		}) {
			eyre::bail!("Tracker credential refs must be globally unique.");
		}
		catalog.push(entry);
	}

	Ok(catalog)
}

/// Read the global fixed account selector, when the operator pinned one.
pub(crate) fn global_fixed_account_selector() -> Result<Option<String>> {
	let config_path = runtime::global_config_path()?;
	let input = match fs::read_to_string(&config_path) {
		Ok(input) => input,
		Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
		Err(error) => {
			eyre::bail!(
				"Failed to read Decodex global config `{}`: {error}",
				config_path.display()
			);
		},
	};
	let document = toml::from_str::<toml::Table>(&input)?;
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

/// Write the global fixed account selector. `None` returns the pool to balanced mode.
#[cfg(test)]
pub(crate) fn write_global_fixed_account_selector(selector: Option<&str>) -> Result<()> {
	let config_path = runtime::global_config_path()?;
	let input = match fs::read_to_string(&config_path) {
		Ok(input) => input,
		Err(error) if error.kind() == ErrorKind::NotFound => String::new(),
		Err(error) => {
			eyre::bail!(
				"Failed to read Decodex global config `{}`: {error}",
				config_path.display()
			);
		},
	};
	let mut document = if input.trim().is_empty() {
		toml::Table::new()
	} else {
		toml::from_str::<toml::Table>(&input)?
	};

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

	let parent = config_path.parent().ok_or_else(|| {
		eyre::eyre!(
			"Decodex global config `{}` must have a parent directory.",
			config_path.display()
		)
	})?;
	let file_name = config_path
		.file_name()
		.and_then(|name| name.to_str())
		.ok_or_else(|| eyre::eyre!("Decodex global config path must end in a valid file name."))?;
	let temp_path = parent.join(format!(".{file_name}.tmp-{}", process::id()));
	let output = toml::to_string_pretty(&document)?;

	fs::create_dir_all(parent)?;
	fs::write(&temp_path, output)?;
	fs::rename(temp_path, &config_path)?;

	Ok(())
}

#[cfg(test)]
pub(super) fn ensure_toml_table<'a>(
	table: &'a mut toml::Table,
	key: &str,
) -> Result<&'a mut toml::Table> {
	if !table.contains_key(key) {
		table.insert(String::from(key), toml::Table::new().into());
	}

	table
		.get_mut(key)
		.and_then(Value::as_table_mut)
		.ok_or_else(|| eyre::eyre!("Decodex global config `{key}` must be a TOML table."))
}
