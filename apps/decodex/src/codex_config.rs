//! Mutations for the user-level Codex configuration file.

mod document;
mod model;
mod paths;

pub(crate) use self::model::CodexFastModeResponse;

use std::path::Path;

use toml_edit::{self, DocumentMut, Item, Table};

use crate::prelude::{Result, eyre};

pub(crate) fn fast_mode_status() -> Result<CodexFastModeResponse> {
	let path = paths::codex_config_path()?;

	fast_mode_status_at_path(&path)
}

pub(crate) fn set_fast_mode(enabled: bool) -> Result<CodexFastModeResponse> {
	let path = paths::codex_config_path()?;

	set_fast_mode_at_path(&path, enabled)
}

fn fast_mode_status_at_path(path: &Path) -> Result<CodexFastModeResponse> {
	let document = document::load_codex_config_document(path)?;

	Ok(CodexFastModeResponse {
		codex_config_path: path.display().to_string(),
		enabled: fast_mode_enabled(&document),
	})
}

fn set_fast_mode_at_path(path: &Path, enabled: bool) -> Result<CodexFastModeResponse> {
	let mut document = document::load_codex_config_document(path)?;
	let root = document.as_table_mut();

	if !root.contains_key("features") {
		root.insert("features", Item::Table(Table::new()));
	}

	let features = root
		.get_mut("features")
		.and_then(Item::as_table_like_mut)
		.ok_or_else(|| eyre::eyre!("`features` in Codex config must be a TOML table."))?;

	features.insert("fast_mode", toml_edit::value(enabled));

	document::write_codex_config_document(path, &document)?;

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
