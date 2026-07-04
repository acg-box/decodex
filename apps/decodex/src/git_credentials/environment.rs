use std::{env, process::Command};

use crate::git_credentials::{
	GIT_CONFIG_ENV_REMOVE_FLOOR, GITHUB_CREDENTIAL_HELPER, GITHUB_HTTPS_URL_BASE,
	GITHUB_SSH_URL_PREFIXES, GitSigningConfig,
};

#[derive(Clone, Default, Eq, PartialEq)]
pub(crate) struct GitCredentialEnvironment {
	github_token_env_var: Option<String>,
	github_token: Option<String>,
	signing_config: GitSigningConfig,
}
impl GitCredentialEnvironment {
	pub(crate) fn with_github_credentials(
		github_token_env_var: String,
		github_token: String,
	) -> Self {
		Self {
			github_token_env_var: Some(github_token_env_var),
			github_token: Some(github_token),
			signing_config: GitSigningConfig::DisableInherited,
		}
	}

	pub(crate) fn with_github_credentials_and_signing_config(
		github_token_env_var: String,
		github_token: String,
		signing_config: GitSigningConfig,
	) -> Self {
		Self {
			github_token_env_var: Some(github_token_env_var),
			github_token: Some(github_token),
			signing_config,
		}
	}

	pub(crate) fn apply_to(&self, command: &mut Command) {
		clear_injected_git_config(command);

		command
			.env("GH_PROMPT_DISABLED", "1")
			.env("GIT_TERMINAL_PROMPT", "0")
			.env("GCM_INTERACTIVE", "never");

		if let Some(github_token) = self.github_token.as_deref() {
			command.env("GH_TOKEN", github_token).env("GITHUB_TOKEN", github_token);

			if let Some(github_token_env_var) = self.github_token_env_var.as_deref() {
				command.env(github_token_env_var, github_token);
			}
		}

		let mut git_config_entries = Vec::new();

		if self.github_token.is_some() {
			command.env_remove("GIT_ASKPASS");
			// Empty helper resets inherited helpers so routed credentials own GitHub auth.
			git_config_entries.push((String::from("credential.helper"), String::new()));
			git_config_entries
				.push((String::from("credential.helper"), String::from(GITHUB_CREDENTIAL_HELPER)));

			for ssh_prefix in GITHUB_SSH_URL_PREFIXES {
				git_config_entries.push((
					format!("url.{GITHUB_HTTPS_URL_BASE}.insteadOf"),
					(*ssh_prefix).to_owned(),
				));
			}
		}

		match &self.signing_config {
			GitSigningConfig::Preserve => {},
			GitSigningConfig::DisableInherited => {
				git_config_entries.push((String::from("commit.gpgsign"), String::from("false")));
				git_config_entries.push((String::from("tag.gpgsign"), String::from("false")));
				git_config_entries.push((String::from("user.signingkey"), String::new()));
			},
			GitSigningConfig::SigningKey(signing_key) => {
				git_config_entries.push((String::from("user.signingkey"), signing_key.clone()));
			},
		}

		if !git_config_entries.is_empty() {
			let git_config_count = git_config_entries.len();

			for (index, (key, value)) in git_config_entries.into_iter().enumerate() {
				command.env(format!("GIT_CONFIG_KEY_{index}"), key);
				command.env(format!("GIT_CONFIG_VALUE_{index}"), value);
			}

			command.env("GIT_CONFIG_COUNT", git_config_count.to_string());
		}
	}
}

pub(crate) fn clear_injected_git_config(command: &mut Command) {
	let config_count = env::var("GIT_CONFIG_COUNT")
		.ok()
		.and_then(|value| value.parse::<usize>().ok())
		.unwrap_or(0);

	command.env_remove("GIT_CONFIG_COUNT");
	command.env_remove("GIT_CONFIG_PARAMETERS");

	for index in 0..config_count.max(GIT_CONFIG_ENV_REMOVE_FLOOR) {
		command.env_remove(format!("GIT_CONFIG_KEY_{index}"));
		command.env_remove(format!("GIT_CONFIG_VALUE_{index}"));
	}
}
