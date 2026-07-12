//! Fail-closed selection of a generation-specific runtime database.

use std::{
	fs::{self, OpenOptions},
	io::Write as _,
	path::{Component, Path, PathBuf},
};

use serde::Deserialize;

use crate::prelude::{Result, eyre};

const RUNTIME_FORMAT_SCHEMA: &str = "decodex/runtime-format/2";
const RUNTIME_FORMAT_MANIFEST: &str = "runtime-format.toml";
const LEGACY_DATABASE: &str = "runtime.sqlite3";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeFormatManifest {
	schema: String,
	generation: u64,
	database_relative_path: PathBuf,
}

pub(super) fn selected_runtime_db_path_from(runtime_root: &Path) -> Result<PathBuf> {
	let legacy_path = runtime_root.join(LEGACY_DATABASE);
	if legacy_path.is_file() {
		eyre::bail!("legacy_runtime_database_requires_offline_archive_and_reset");
	}
	let manifest_path = runtime_root.join(RUNTIME_FORMAT_MANIFEST);
	let manifest_bytes = fs::read(&manifest_path).map_err(|error| {
		eyre::eyre!(
			"runtime_format_manifest_unavailable:{}:{error}",
			manifest_path.display()
		)
	})?;
	let manifest_text = std::str::from_utf8(&manifest_bytes)
		.map_err(|_| eyre::eyre!("runtime_format_manifest_not_utf8"))?;
	let manifest: RuntimeFormatManifest = toml::from_str(manifest_text)
		.map_err(|_| eyre::eyre!("runtime_format_manifest_invalid"))?;
	if manifest.schema != RUNTIME_FORMAT_SCHEMA || manifest.generation == 0 {
		eyre::bail!("runtime_format_manifest_unsupported");
	}
	validate_database_relative_path(&manifest.database_relative_path, manifest.generation)?;
	let selected = runtime_root.join(&manifest.database_relative_path);
	if selected == legacy_path {
		eyre::bail!("runtime_format_manifest_selects_legacy_database");
	}
	if !selected.is_file() {
		eyre::bail!("runtime_format_selected_database_unavailable");
	}
	Ok(selected)
}

pub(super) fn publish_runtime_generation_from(
	runtime_root: &Path,
	generation: u64,
) -> Result<PathBuf> {
	if generation == 0 {
		eyre::bail!("runtime_generation_zero");
	}
	let legacy_path = runtime_root.join(LEGACY_DATABASE);
	if !legacy_path.is_dir() {
		eyre::bail!("legacy_runtime_tombstone_missing");
	}
	let relative_database = PathBuf::from("generations")
		.join(generation.to_string())
		.join(LEGACY_DATABASE);
	let database = runtime_root.join(&relative_database);
	if !database.is_file() {
		eyre::bail!("runtime_generation_database_not_prepared");
	}
	let manifest = runtime_root.join(RUNTIME_FORMAT_MANIFEST);
	if manifest.exists() {
		eyre::bail!("runtime_format_manifest_already_published");
	}
	let temp = runtime_root.join(format!(".runtime-format.toml.{}.tmp", std::process::id()));
	let body = format!(
		"schema = \"{RUNTIME_FORMAT_SCHEMA}\"\ngeneration = {generation}\ndatabase_relative_path = \"{}\"\n",
		relative_database.display()
	);
	let mut file = OpenOptions::new().create_new(true).write(true).open(&temp)?;
	file.write_all(body.as_bytes())?;
	file.sync_all()?;
	drop(file);
	if let Err(error) = fs::rename(&temp, &manifest) {
		let _ = fs::remove_file(&temp);
		return Err(error.into());
	}
	OpenOptions::new().read(true).open(runtime_root)?.sync_all()?;
	selected_runtime_db_path_from(runtime_root)
}

fn validate_database_relative_path(path: &Path, generation: u64) -> Result<()> {
	if path.is_absolute()
		|| path.components().any(|component| !matches!(component, Component::Normal(_)))
	{
		eyre::bail!("runtime_format_database_path_not_confined");
	}
	let expected = PathBuf::from("generations")
		.join(generation.to_string())
		.join(LEGACY_DATABASE);
	if path != expected {
		eyre::bail!("runtime_format_database_path_generation_mismatch");
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use std::fs;

	use tempfile::TempDir;

	use super::*;

	#[test]
	fn lane_authority_v2_c1_selects_only_manifest_generation_database() {
		let temp = TempDir::new().expect("tempdir");
		fs::create_dir_all(temp.path().join("generations/7")).expect("generation dir");
		fs::write(temp.path().join("generations/7/runtime.sqlite3"), b"prepared")
			.expect("database");
		fs::write(
			temp.path().join(RUNTIME_FORMAT_MANIFEST),
			"schema = \"decodex/runtime-format/2\"\ngeneration = 7\ndatabase_relative_path = \"generations/7/runtime.sqlite3\"\n",
		)
		.expect("manifest");
		assert_eq!(
			selected_runtime_db_path_from(temp.path()).expect("selection"),
			temp.path().join("generations/7/runtime.sqlite3")
		);
	}

	#[test]
	fn lane_authority_v2_c1_rejects_legacy_database_even_with_manifest() {
		let temp = TempDir::new().expect("tempdir");
		fs::write(temp.path().join(LEGACY_DATABASE), b"legacy").expect("legacy");
		fs::write(
			temp.path().join(RUNTIME_FORMAT_MANIFEST),
			"schema = \"decodex/runtime-format/2\"\ngeneration = 1\ndatabase_relative_path = \"generations/1/runtime.sqlite3\"\n",
		)
		.expect("manifest");
		assert!(
			selected_runtime_db_path_from(temp.path())
				.expect_err("legacy refusal")
				.to_string()
				.contains("legacy_runtime_database")
		);
	}

	#[test]
	fn lane_authority_v2_c1_publishes_prepared_generation_last_and_once() {
		let temp = TempDir::new().expect("tempdir");
		fs::create_dir(temp.path().join(LEGACY_DATABASE)).expect("tombstone");
		fs::create_dir_all(temp.path().join("generations/3")).expect("generation dir");
		let database = temp.path().join("generations/3/runtime.sqlite3");
		fs::write(&database, b"prepared").expect("database");
		assert_eq!(publish_runtime_generation_from(temp.path(), 3).expect("publish"), database);
		assert!(publish_runtime_generation_from(temp.path(), 3).is_err());
		assert!(
			!temp
				.path()
				.join(format!(".runtime-format.toml.{}.tmp", std::process::id()))
				.exists()
		);
	}

	#[test]
	fn lane_authority_v2_c1_refuses_publish_without_tombstone_or_prepared_database() {
		let temp = TempDir::new().expect("tempdir");
		assert!(publish_runtime_generation_from(temp.path(), 1).is_err());
		fs::create_dir(temp.path().join(LEGACY_DATABASE)).expect("tombstone");
		assert!(publish_runtime_generation_from(temp.path(), 1).is_err());
		assert!(!temp.path().join(RUNTIME_FORMAT_MANIFEST).exists());
	}

	#[test]
	fn lane_authority_v2_c1_rejects_manifest_path_escape_and_generation_drift() {
		for path in ["../runtime.sqlite3", "/tmp/runtime.sqlite3", "generations/2/runtime.sqlite3"] {
			let temp = TempDir::new().expect("tempdir");
			fs::write(
				temp.path().join(RUNTIME_FORMAT_MANIFEST),
				format!(
					"schema = \"decodex/runtime-format/2\"\ngeneration = 1\ndatabase_relative_path = \"{path}\"\n"
				),
			)
			.expect("manifest");
			assert!(selected_runtime_db_path_from(temp.path()).is_err(), "accepted {path}");
		}
	}
}
