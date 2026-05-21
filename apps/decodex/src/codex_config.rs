//! Mutations for the user-level Codex configuration file.

#[cfg(unix)] use std::os::unix::fs::PermissionsExt as _;
use std::{
	env, fs,
	io::ErrorKind,
	path::{Path, PathBuf},
	process,
};

use serde::Serialize;
use toml_edit::{self, DocumentMut, Item, Table};

use crate::prelude::{Result, eyre};

#[derive(Debug, Serialize)]
pub(crate) struct CodexFastModeResponse {
	pub(crate) codex_config_path: String,
	pub(crate) enabled: bool,
}

pub(crate) fn fast_mode_status() -> Result<CodexFastModeResponse> {
	let path = codex_config_path()?;

	fast_mode_status_at_path(&path)
}

pub(crate) fn set_fast_mode(enabled: bool) -> Result<CodexFastModeResponse> {
	let path = codex_config_path()?;

	set_fast_mode_at_path(&path, enabled)
}

fn fast_mode_status_at_path(path: &Path) -> Result<CodexFastModeResponse> {
	let document = load_codex_config_document(path)?;

	Ok(CodexFastModeResponse {
		codex_config_path: path.display().to_string(),
		enabled: fast_mode_enabled(&document),
	})
}

fn set_fast_mode_at_path(path: &Path, enabled: bool) -> Result<CodexFastModeResponse> {
	let mut document = load_codex_config_document(path)?;
	let root = document.as_table_mut();

	if !root.contains_key("features") {
		root.insert("features", Item::Table(Table::new()));
	}

	let features = root
		.get_mut("features")
		.and_then(Item::as_table_like_mut)
		.ok_or_else(|| eyre::eyre!("`features` in Codex config must be a TOML table."))?;

	features.insert("fast_mode", toml_edit::value(enabled));

	write_codex_config_document(path, &document)?;

	Ok(CodexFastModeResponse { codex_config_path: path.display().to_string(), enabled })
}

fn fast_mode_enabled(document: &DocumentMut) -> bool {
	document
		.get("features")
		.and_then(Item::as_table_like)
		.and_then(|features| features.get("fast_mode"))
		.and_then(Item::as_bool)
		.unwrap_or(false)
}

fn load_codex_config_document(path: &Path) -> Result<DocumentMut> {
	let input = match fs::read_to_string(path) {
		Ok(input) => input,
		Err(error) if error.kind() == ErrorKind::NotFound => return Ok(DocumentMut::new()),
		Err(error) => {
			eyre::bail!("Failed to read Codex config `{}`: {error}", path.display());
		},
	};

	if input.trim().is_empty() {
		return Ok(DocumentMut::new());
	}

	input
		.parse::<DocumentMut>()
		.map_err(|error| eyre::eyre!("Codex config `{}` is invalid TOML: {error}", path.display()))
}

fn write_codex_config_document(path: &Path, document: &DocumentMut) -> Result<()> {
	let parent = path.parent().ok_or_else(|| {
		eyre::eyre!("Codex config path `{}` must have a parent directory.", path.display())
	})?;
	let file_name = path
		.file_name()
		.and_then(|name| name.to_str())
		.ok_or_else(|| eyre::eyre!("Codex config path must end in a valid file name."))?;
	let temp_path = parent.join(format!(".{file_name}.tmp-{}", process::id()));
	let mut output = document.to_string();

	if !output.ends_with('\n') {
		output.push('\n');
	}

	fs::create_dir_all(parent)?;
	fs::write(&temp_path, output)?;

	secure_config_file(&temp_path)?;

	fs::rename(temp_path, path)?;

	secure_config_file(path)?;

	Ok(())
}

fn codex_config_path() -> Result<PathBuf> {
	Ok(codex_home_dir()?.join("config.toml"))
}

fn codex_home_dir() -> Result<PathBuf> {
	if let Some(codex_home) = env::var_os("CODEX_HOME") {
		let path = PathBuf::from(codex_home);

		if !path.as_os_str().is_empty() {
			return Ok(path);
		}
	}

	let Some(home) = env::var_os("HOME") else {
		eyre::bail!("Failed to resolve `$HOME` for the Codex config path.");
	};

	Ok(PathBuf::from(home).join(".codex"))
}

fn secure_config_file(path: &Path) -> Result<()> {
	#[cfg(unix)]
	{
		let mut permissions = fs::metadata(path)?.permissions();

		permissions.set_mode(0o600);

		fs::set_permissions(path, permissions)?;
	}

	Ok(())
}

#[cfg(test)]
mod tests {
	use std::fs;

	#[test]
	fn missing_config_reports_fast_mode_disabled() {
		let temp = tempfile::tempdir().expect("tempdir should be created");
		let path = temp.path().join("config.toml");
		let response = super::fast_mode_status_at_path(&path).expect("status should load");

		assert_eq!(response.codex_config_path, path.display().to_string());
		assert!(!response.enabled);
	}

	#[test]
	fn set_fast_mode_creates_features_table_and_overwrites_value() {
		let temp = tempfile::tempdir().expect("tempdir should be created");
		let path = temp.path().join("config.toml");
		let enabled = super::set_fast_mode_at_path(&path, true).expect("fast mode should enable");

		assert!(enabled.enabled);

		let disabled =
			super::set_fast_mode_at_path(&path, false).expect("fast mode should disable");
		let output = fs::read_to_string(&path).expect("config should be written");

		assert!(!disabled.enabled);
		assert!(output.contains("[features]"));
		assert!(output.contains("fast_mode = false"));
	}

	#[test]
	fn set_fast_mode_preserves_unrelated_config() {
		let temp = tempfile::tempdir().expect("tempdir should be created");
		let path = temp.path().join("config.toml");

		fs::write(
			&path,
			"# local Codex settings\nmodel = \"gpt-5.2\"\n\n[features]\nplugins = true\nfast_mode = false\n",
		)
		.expect("seed config should write");
		super::set_fast_mode_at_path(&path, true).expect("fast mode should enable");

		let output = fs::read_to_string(&path).expect("config should be written");

		assert!(output.contains("# local Codex settings"));
		assert!(output.contains("model = \"gpt-5.2\""));
		assert!(output.contains("plugins = true"));
		assert!(output.contains("fast_mode = true"));
	}

	#[test]
	fn set_fast_mode_rejects_non_table_features() {
		let temp = tempfile::tempdir().expect("tempdir should be created");
		let path = temp.path().join("config.toml");

		fs::write(&path, "features = true\n").expect("seed config should write");

		let error =
			super::set_fast_mode_at_path(&path, true).expect_err("features must be a table");

		assert!(error.to_string().contains("`features` in Codex config must be a TOML table"));
	}
}
