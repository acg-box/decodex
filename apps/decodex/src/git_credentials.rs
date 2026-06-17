use std::{env, path::Path, process::Command};

use crate::prelude::{Result, eyre};

const GITHUB_HTTPS_URL_BASE: &str = "https://github.com/";
const GITHUB_SSH_URL_PREFIXES: &[&str] = &[
	"git@github.com:",
	"git@github.com-x:",
	"git@github.com-y:",
	"ssh://git@github.com/",
	"ssh://git@github.com-x/",
	"ssh://git@github.com-y/",
];
const GIT_CONFIG_ENV_REMOVE_FLOOR: usize = 64;
const GITHUB_CREDENTIAL_HELPER: &str = concat!(
	"!f() { ",
	"test \"$1\" = get || exit 0; ",
	"protocol=; host=; ",
	"while IFS= read -r line; do ",
	"test -n \"$line\" || break; ",
	"case \"$line\" in ",
	"protocol=*) protocol=${line#protocol=} ;; ",
	"host=*) host=${line#host=} ;; ",
	"esac; ",
	"done; ",
	"test \"$protocol\" = https || exit 0; ",
	"test \"$host\" = github.com || exit 0; ",
	"test -n \"$GH_TOKEN\" || exit 0; ",
	"printf '%s\\n' username=x-access-token password=\"$GH_TOKEN\"; ",
	"}; f",
);

#[derive(Clone, Copy)]
pub(crate) struct GitCredentialSource<'a> {
	token_env_var: &'a str,
	token: &'a str,
}
impl<'a> GitCredentialSource<'a> {
	pub(crate) fn new(token_env_var: &'a str, token: &'a str) -> Self {
		Self { token_env_var, token }
	}

	pub(crate) fn materialize_github_credentials(self) -> GitCredentialEnvironment {
		GitCredentialEnvironment::with_github_credentials(
			self.token_env_var.to_owned(),
			self.token.to_owned(),
		)
	}
}

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

#[derive(Clone, Default, Eq, PartialEq)]
pub(crate) enum GitSigningConfig {
	#[default]
	Preserve,
	DisableInherited,
	SigningKey(String),
}
impl GitSigningConfig {
	pub(crate) fn from_local_git_config(repo_root: &Path) -> Result<Self> {
		let output = Command::new("git")
			.arg("-C")
			.arg(repo_root)
			.args(["config", "--local", "--includes", "--get", "user.signingkey"])
			.output()?;

		if output.status.success() {
			let signing_key = String::from_utf8_lossy(&output.stdout).trim().to_owned();

			return if signing_key.is_empty() {
				Ok(Self::DisableInherited)
			} else {
				Ok(Self::SigningKey(signing_key))
			};
		}
		if output.status.code() == Some(1) {
			return Ok(Self::Preserve);
		}

		let stderr = String::from_utf8_lossy(&output.stderr);

		eyre::bail!(
			"Failed to inspect local Git signing key in `{}`: {}",
			repo_root.display(),
			stderr.trim()
		);
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

#[cfg(test)]
mod tests {
	use std::{
		ffi::OsStr,
		io::Write as _,
		process::{Command, Stdio},
	};

	use crate::git_credentials::GitCredentialEnvironment;

	#[test]
	fn apply_to_scrubs_inherited_git_config_injection() {
		let mut command = Command::new("git");

		command
			.env("GIT_CONFIG_PARAMETERS", "commit.gpgsign=true")
			.env("GIT_CONFIG_COUNT", "1")
			.env("GIT_CONFIG_KEY_0", "commit.gpgsign")
			.env("GIT_CONFIG_VALUE_0", "true");

		GitCredentialEnvironment::default().apply_to(&mut command);

		assert_env_removed(&command, "GIT_CONFIG_PARAMETERS");
		assert_env_removed(&command, "GIT_CONFIG_COUNT");
		assert_env_removed(&command, "GIT_CONFIG_KEY_0");
		assert_env_removed(&command, "GIT_CONFIG_VALUE_0");
	}

	#[test]
	fn inline_github_credentials_supply_only_github_https_credentials() {
		let github = credential_fill_stdout("github.com")
			.expect("github credential helper should return credentials");

		assert!(github.contains("username=x-access-token"));
		assert!(github.contains("password=secret-token-value"));

		let foreign = credential_fill_stdout("github.com.evil")
			.expect_err("foreign host should not receive credentials");

		assert!(!foreign.contains("secret-token-value"));
	}

	fn credential_fill_stdout(host: &str) -> std::result::Result<String, String> {
		let mut command = Command::new("git");

		GitCredentialEnvironment::with_github_credentials(
			String::from("GITHUB_PAT_Y"),
			String::from("secret-token-value"),
		)
		.apply_to(&mut command);

		let mut child = command
			.args(["credential", "fill"])
			.stdin(Stdio::piped())
			.stdout(Stdio::piped())
			.stderr(Stdio::piped())
			.spawn()
			.expect("git credential fill should spawn");

		child
			.stdin
			.as_mut()
			.expect("stdin should be piped")
			.write_all(format!("protocol=https\nhost={host}\n\n").as_bytes())
			.expect("credential input should write");

		let output = child.wait_with_output().expect("credential fill should exit");
		let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
		let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

		if output.status.success() { Ok(stdout) } else { Err(format!("{stdout}{stderr}")) }
	}

	fn assert_env_removed(command: &Command, name: &str) {
		let target = OsStr::new(name);

		assert!(
			command.get_envs().any(|(key, value)| key == target && value.is_none()),
			"`{name}` should be explicitly removed from child environment"
		);
	}
}
