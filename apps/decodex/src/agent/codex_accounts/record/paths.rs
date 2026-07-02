use std::{env, path::PathBuf};

use crate::{
	agent::codex_accounts::{DEFAULT_PROFILE_ENDPOINT, DEFAULT_USAGE_ENDPOINT},
	prelude::{Result, eyre},
};

pub(in crate::agent::codex_accounts) fn default_codex_auth_json_path() -> Result<PathBuf> {
	if let Some(codex_home) =
		env::var_os("CODEX_HOME").map(PathBuf::from).filter(|path| !path.as_os_str().is_empty())
	{
		return Ok(codex_home.join("auth.json"));
	}

	let Some(home) = env::var_os("HOME") else {
		eyre::bail!("Failed to resolve `$HOME` for the Codex auth JSON path.");
	};

	Ok(PathBuf::from(home).join(".codex").join("auth.json"))
}

pub(in crate::agent::codex_accounts) fn default_profile_endpoint_for_usage_endpoint(
	usage_endpoint: &str,
) -> Option<String> {
	(usage_endpoint == DEFAULT_USAGE_ENDPOINT).then(|| DEFAULT_PROFILE_ENDPOINT.to_owned())
}
