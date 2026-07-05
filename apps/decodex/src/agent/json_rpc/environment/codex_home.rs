use std::{
	env,
	ffi::OsString,
	path::{Path, PathBuf},
	process::Command,
};

use color_eyre::Report;

use crate::{agent::json_rpc::errors::AppServerHomePreflightFailure, prelude::Result};

const CODEX_HOME_ENV_VAR: &str = "CODEX_HOME";
const CODEX_SQLITE_HOME_ENV_VAR: &str = "CODEX_SQLITE_HOME";
const CODEX_HOME_DIR_NAME: &str = ".codex";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResolvedAppServerCodexHomeEnv {
	codex_home: PathBuf,
	sqlite_home: PathBuf,
}
impl ResolvedAppServerCodexHomeEnv {
	pub(crate) fn new(codex_home: PathBuf, sqlite_home: PathBuf) -> Result<Self> {
		validate_codex_home_path(CODEX_HOME_ENV_VAR, &codex_home)?;
		validate_codex_home_path(CODEX_SQLITE_HOME_ENV_VAR, &sqlite_home)?;

		Ok(Self { codex_home, sqlite_home })
	}

	pub(crate) fn codex_home(&self) -> &Path {
		&self.codex_home
	}

	#[cfg(test)]
	pub(in crate::agent::json_rpc) fn sqlite_home(&self) -> &Path {
		&self.sqlite_home
	}

	pub(in crate::agent::json_rpc::environment) fn apply_to(
		&self,
		command: &mut Command,
	) -> Result<()> {
		let codex_home = path_env_value(CODEX_HOME_ENV_VAR, &self.codex_home)?;
		let sqlite_home = path_env_value(CODEX_SQLITE_HOME_ENV_VAR, &self.sqlite_home)?;

		command.env_remove(CODEX_HOME_ENV_VAR);
		command.env_remove(CODEX_SQLITE_HOME_ENV_VAR);
		command.env(CODEX_HOME_ENV_VAR, codex_home);
		command.env(CODEX_SQLITE_HOME_ENV_VAR, sqlite_home);

		Ok(())
	}
}

pub(crate) fn resolve_shared_codex_home_env_from_home(
	home: Option<OsString>,
) -> Result<ResolvedAppServerCodexHomeEnv> {
	let Some(home) = home else {
		return Err(Report::new(AppServerHomePreflightFailure::resolution_failed(String::from(
			"app_server_preflight_failed: HOME is not set, so Decodex cannot resolve the shared Codex home for app-server dispatch.",
		))));
	};
	let home = PathBuf::from(home);

	if home.as_os_str().is_empty() {
		return Err(Report::new(AppServerHomePreflightFailure::resolution_failed(String::from(
			"app_server_preflight_failed: HOME is empty, so Decodex cannot resolve the shared Codex home for app-server dispatch.",
		))));
	}
	if !home.is_absolute() {
		return Err(Report::new(AppServerHomePreflightFailure::resolution_failed(format!(
			"app_server_preflight_failed: HOME `{}` is not absolute, so Decodex cannot resolve the shared Codex home for app-server dispatch.",
			home.display()
		))));
	}

	let codex_home = home.join(CODEX_HOME_DIR_NAME);

	ResolvedAppServerCodexHomeEnv::new(codex_home.clone(), codex_home)
}

pub(in crate::agent::json_rpc::environment) fn resolve_shared_codex_home_env()
-> Result<ResolvedAppServerCodexHomeEnv> {
	resolve_shared_codex_home_env_from_home(env::var_os("HOME"))
}

fn validate_codex_home_path(name: &str, path: &Path) -> Result<()> {
	if path.as_os_str().is_empty() {
		return Err(Report::new(AppServerHomePreflightFailure::resolution_failed(format!(
			"app_server_preflight_failed: {name} resolved to an empty path."
		))));
	}
	if !path.is_absolute() {
		return Err(Report::new(AppServerHomePreflightFailure::resolution_failed(format!(
			"app_server_preflight_failed: {name} `{}` is not absolute.",
			path.display()
		))));
	}

	path_env_value(name, path).map(|_| ())
}

fn path_env_value(name: &str, path: &Path) -> Result<String> {
	path.to_str().map(str::to_owned).ok_or_else(|| {
		Report::new(AppServerHomePreflightFailure::resolution_failed(format!(
			"app_server_preflight_failed: {name} `{}` is not valid UTF-8.",
			path.display()
		)))
	})
}
