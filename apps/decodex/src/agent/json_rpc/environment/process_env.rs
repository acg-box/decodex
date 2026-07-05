use std::process::Command;

use crate::{
	agent::json_rpc::environment::codex_home::{self, ResolvedAppServerCodexHomeEnv},
	git_credentials::{GitCredentialEnvironment, GitSigningConfig},
	prelude::Result,
};

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
			AppServerCodexHomePolicy::SharedDefault => codex_home::resolve_shared_codex_home_env(),
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
	pub(crate) fn with_codex_home_for_test(home_env: ResolvedAppServerCodexHomeEnv) -> Self {
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

#[derive(Clone, Eq, PartialEq)]
enum AppServerCodexHomePolicy {
	SharedDefault,
	#[cfg(test)]
	Explicit(ResolvedAppServerCodexHomeEnv),
}
