#[allow(clippy::wildcard_imports)] use super::*;

use agent::AppServerProcessEnv;
use git_credentials::GitSigningConfig;

pub(crate) struct AgentGitCredentialEnvironment {
	process_env: AppServerProcessEnv,
}
impl AgentGitCredentialEnvironment {
	pub(crate) fn process_env(&self) -> &AppServerProcessEnv {
		&self.process_env
	}
}

pub(crate) fn prepare_agent_git_credentials(
	project: &ServiceConfig,
	run_id: &str,
	worktree_path: &Path,
) -> Result<AgentGitCredentialEnvironment> {
	let github_token = project.github().resolve_token().map_err(|error| {
		Report::new(AgentGitCredentialsUnavailable {
			run_id: run_id.to_owned(),
			token_env_var: project.github().token_env_var().to_owned(),
		})
		.wrap_err(error)
	})?;
	let signing_config = GitSigningConfig::from_local_git_config(worktree_path)?;

	Ok(AgentGitCredentialEnvironment {
		process_env: AppServerProcessEnv::with_github_credentials_and_signing_config(
			project.github().token_env_var().to_owned(),
			github_token,
			signing_config,
		),
	})
}
