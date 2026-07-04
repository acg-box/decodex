use std::{env, path::PathBuf};

use crate::prelude::{Result, eyre};

pub(in crate::codex_config) fn codex_config_path() -> Result<PathBuf> {
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
