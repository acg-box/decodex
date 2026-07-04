use std::{
	fs,
	path::{Component, Path, PathBuf},
};

use crate::prelude::{Result, eyre};

pub(in crate::config) const PROJECT_CONFIG_FILE_NAME: &str = "project.toml";

pub(in crate::config) fn canonicalize_path_best_effort(path: &Path) -> PathBuf {
	fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

pub(in crate::config) fn resolve_project_config_file_path(path: &Path) -> Result<PathBuf> {
	let metadata = fs::metadata(path).map_err(|error| {
		eyre::eyre!("Failed to inspect Decodex project config path `{}`: {error}", path.display())
	})?;

	if metadata.is_dir() {
		return Ok(path.join(PROJECT_CONFIG_FILE_NAME));
	}
	if path.file_name().and_then(|name| name.to_str()) == Some(PROJECT_CONFIG_FILE_NAME) {
		return Ok(path.to_path_buf());
	}

	eyre::bail!(
		"Decodex project config must be a project directory or `{PROJECT_CONFIG_FILE_NAME}` file: `{}`.",
		path.display()
	);
}

pub(in crate::config) fn config_parent_dir(config_path: &Path) -> Result<PathBuf> {
	let canonical_path = fs::canonicalize(config_path)?;
	let Some(parent) = canonical_path.parent() else {
		eyre::bail!("Config path `{}` must have a parent directory.", config_path.display());
	};

	Ok(parent.to_path_buf())
}

pub(in crate::config) fn resolve_relative_path(base: &Path, path: &Path) -> PathBuf {
	let resolved = if path.is_absolute() { path.to_path_buf() } else { base.join(path) };

	normalize_path(&resolved)
}

fn normalize_path(path: &Path) -> PathBuf {
	let mut normalized = PathBuf::new();

	for component in path.components() {
		match component {
			Component::CurDir => {},
			Component::ParentDir => match normalized.components().next_back() {
				Some(Component::Normal(_)) => {
					normalized.pop();
				},
				Some(Component::RootDir | Component::Prefix(_)) => {},
				Some(Component::ParentDir) | None => normalized.push(component.as_os_str()),
				Some(Component::CurDir) => {},
			},
			_ => normalized.push(component.as_os_str()),
		}
	}

	if normalized.as_os_str().is_empty() { PathBuf::from(".") } else { normalized }
}
