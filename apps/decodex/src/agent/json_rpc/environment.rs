use std::{
	env,
	ffi::OsString,
	path::{Path, PathBuf},
	process::{Command, Stdio},
};

use color_eyre::Report;

use crate::{
	agent::json_rpc::errors::AppServerHomePreflightFailure,
	git_credentials::{GitCredentialEnvironment, GitSigningConfig},
	prelude::Result,
};

pub(super) const APP_SERVER_STDERR_TAIL_LINES: usize = 20;

const CODEX_APP_SERVER_BINARY: &str = "codex";
const CODEX_HOME_ENV_VAR: &str = "CODEX_HOME";
const CODEX_SQLITE_HOME_ENV_VAR: &str = "CODEX_SQLITE_HOME";
const CODEX_HOME_DIR_NAME: &str = ".codex";

#[derive(Clone, Eq, PartialEq)]
pub(crate) struct AppServerProcessEnv {
	git: GitCredentialEnvironment,
	codex_home_policy: AppServerCodexHomePolicy,
}
impl AppServerProcessEnv {
	#[cfg(test)]
	pub(crate) fn with_github_credentials(
		github_token_env_var: String,
		github_token: String,
	) -> Self {
		Self {
			git: GitCredentialEnvironment::with_github_credentials(
				github_token_env_var,
				github_token,
			),
			codex_home_policy: AppServerCodexHomePolicy::SharedDefault,
		}
	}

	pub(crate) fn with_github_credentials_and_signing_config(
		github_token_env_var: String,
		github_token: String,
		signing_config: GitSigningConfig,
	) -> Self {
		Self {
			git: GitCredentialEnvironment::with_github_credentials_and_signing_config(
				github_token_env_var,
				github_token,
				signing_config,
			),
			codex_home_policy: AppServerCodexHomePolicy::SharedDefault,
		}
	}

	pub(crate) fn resolve_codex_home_env(&self) -> Result<ResolvedAppServerCodexHomeEnv> {
		match &self.codex_home_policy {
			AppServerCodexHomePolicy::SharedDefault => resolve_shared_codex_home_env(),
			#[cfg(test)]
			AppServerCodexHomePolicy::Explicit(home_env) => Ok(home_env.clone()),
		}
	}

	pub(crate) fn apply_to(&self, command: &mut Command) -> Result<ResolvedAppServerCodexHomeEnv> {
		self.git.apply_to(command);

		let codex_home_env = self.resolve_codex_home_env()?;

		codex_home_env.apply_to(command)?;

		Ok(codex_home_env)
	}

	#[cfg(test)]
	pub(super) fn with_codex_home_for_test(home_env: ResolvedAppServerCodexHomeEnv) -> Self {
		Self {
			git: GitCredentialEnvironment::default(),
			codex_home_policy: AppServerCodexHomePolicy::Explicit(home_env),
		}
	}
}

impl Default for AppServerProcessEnv {
	fn default() -> Self {
		Self {
			git: GitCredentialEnvironment::default(),
			codex_home_policy: AppServerCodexHomePolicy::SharedDefault,
		}
	}
}

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
	pub(super) fn sqlite_home(&self) -> &Path {
		&self.sqlite_home
	}

	fn apply_to(&self, command: &mut Command) -> Result<()> {
		let codex_home = path_env_value(CODEX_HOME_ENV_VAR, &self.codex_home)?;
		let sqlite_home = path_env_value(CODEX_SQLITE_HOME_ENV_VAR, &self.sqlite_home)?;

		command.env_remove(CODEX_HOME_ENV_VAR);
		command.env_remove(CODEX_SQLITE_HOME_ENV_VAR);
		command.env(CODEX_HOME_ENV_VAR, codex_home);
		command.env(CODEX_SQLITE_HOME_ENV_VAR, sqlite_home);

		Ok(())
	}
}

#[derive(Clone, Eq, PartialEq)]
enum AppServerCodexHomePolicy {
	SharedDefault,
	#[cfg(test)]
	Explicit(ResolvedAppServerCodexHomeEnv),
}

pub(crate) fn app_server_command_program() -> PathBuf {
	app_server_command_program_from_env(env::var_os("PATH"), env::var_os("HOME"))
}

pub(super) fn resolve_shared_codex_home_env_from_home(
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

pub(super) fn configure_app_server_command(
	command: &mut Command,
	listen: &str,
	process_env: &AppServerProcessEnv,
) -> Result<ResolvedAppServerCodexHomeEnv> {
	command
		.args(["app-server", "--listen", listen])
		.stdin(Stdio::piped())
		.stdout(Stdio::piped())
		.stderr(Stdio::piped());

	process_env.apply_to(command)
}

pub(super) fn app_server_command_program_from_env(
	path_env: Option<OsString>,
	home: Option<OsString>,
) -> PathBuf {
	if let Some(path_env) = path_env {
		for path_entry in env::split_paths(&path_env) {
			let candidate = path_entry.join(CODEX_APP_SERVER_BINARY);

			if candidate.is_file() {
				return candidate;
			}
		}
	}
	if let Some(home) = home {
		let home = PathBuf::from(home);

		for relative_candidate in
			[[".local", "bin", CODEX_APP_SERVER_BINARY], [".cargo", "bin", CODEX_APP_SERVER_BINARY]]
		{
			let candidate = relative_candidate
				.iter()
				.fold(home.clone(), |path, component| path.join(*component));

			if candidate.is_file() {
				return candidate;
			}
		}
	}

	PathBuf::from(CODEX_APP_SERVER_BINARY)
}

fn resolve_shared_codex_home_env() -> Result<ResolvedAppServerCodexHomeEnv> {
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
