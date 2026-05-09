#[cfg(unix)] use std::os::unix::fs::PermissionsExt as _;
use std::{
	fs,
	io::ErrorKind,
	path::{Path, PathBuf},
	process::{self, Command},
	sync::atomic::{AtomicU64, Ordering},
};

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
static NEXT_ASKPASS_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy)]
pub(crate) struct GitCredentialSource<'a> {
	token_env_var: &'a str,
	token: &'a str,
	askpass_root: &'a Path,
}
impl<'a> GitCredentialSource<'a> {
	pub(crate) fn new(token_env_var: &'a str, token: &'a str, askpass_root: &'a Path) -> Self {
		Self { token_env_var, token, askpass_root }
	}

	pub(crate) fn materialize_github_askpass(
		self,
		label: &str,
	) -> Result<(GitCredentialEnvironment, GitAskpassGuard)> {
		let askpass_path = scoped_github_askpass_path(self.askpass_root, label);
		let askpass_guard = GitAskpassGuard::create(askpass_path.clone())?;
		let git_env = GitCredentialEnvironment::with_github_credentials(
			self.token_env_var.to_owned(),
			self.token.to_owned(),
			askpass_path,
		);

		Ok((git_env, askpass_guard))
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

#[derive(Clone, Default, Eq, PartialEq)]
pub(crate) struct GitCredentialEnvironment {
	github_token_env_var: Option<String>,
	github_token: Option<String>,
	git_askpass_path: Option<PathBuf>,
	signing_config: GitSigningConfig,
}
impl GitCredentialEnvironment {
	pub(crate) fn with_github_credentials(
		github_token_env_var: String,
		github_token: String,
		git_askpass_path: PathBuf,
	) -> Self {
		Self {
			github_token_env_var: Some(github_token_env_var),
			github_token: Some(github_token),
			git_askpass_path: Some(git_askpass_path),
			signing_config: GitSigningConfig::DisableInherited,
		}
	}

	pub(crate) fn with_github_credentials_and_signing_config(
		github_token_env_var: String,
		github_token: String,
		git_askpass_path: PathBuf,
		signing_config: GitSigningConfig,
	) -> Self {
		Self {
			github_token_env_var: Some(github_token_env_var),
			github_token: Some(github_token),
			git_askpass_path: Some(git_askpass_path),
			signing_config,
		}
	}

	pub(crate) fn apply_to(&self, command: &mut Command) {
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
		if let Some(git_askpass_path) = self.git_askpass_path.as_deref() {
			command.env("GIT_ASKPASS", git_askpass_path);
		}

		let mut git_config_entries = Vec::new();

		if self.github_token.is_some() && self.git_askpass_path.is_some() {
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

pub(crate) struct GitAskpassGuard {
	path: PathBuf,
}
impl GitAskpassGuard {
	pub(crate) fn create(path: PathBuf) -> Result<Self> {
		write_github_askpass_helper(&path)?;

		Ok(Self { path })
	}
}
impl Drop for GitAskpassGuard {
	fn drop(&mut self) {
		if let Err(error) = fs::remove_file(&self.path)
			&& error.kind() != ErrorKind::NotFound
		{
			tracing::warn!(
				?error,
				askpass_path = %self.path.display(),
				"Failed to remove Git askpass helper."
			);
		}
	}
}

pub(crate) fn scoped_github_askpass_path(root: &Path, label: &str) -> PathBuf {
	let safe_label = sanitize_path_component(label);
	let id = NEXT_ASKPASS_ID.fetch_add(1, Ordering::Relaxed);

	root.join(format!(".decodex-git-askpass-{safe_label}-{}-{id}.sh", process::id()))
}

pub(crate) fn write_github_askpass_helper(path: &Path) -> Result<()> {
	if let Some(parent) = path.parent() {
		fs::create_dir_all(parent)?;
	}

	fs::write(
		path,
		"#!/bin/sh\ncase \"$1\" in\n  *https://github.com:*|*https://github.com/*|*https://github.com\\'*|*https://*@github.com:*|*https://*@github.com/*|*https://*@github.com\\'*) ;;\n  *) exit 1 ;;\nesac\ncase \"$1\" in\n  *Username*|*username*) printf '%s\\n' 'x-access-token' ;;\n  *Password*|*password*) printf '%s\\n' \"$GH_TOKEN\" ;;\n  *) exit 1 ;;\nesac\n",
	)?;

	#[cfg(unix)]
	{
		let mut permissions = fs::metadata(path)?.permissions();

		permissions.set_mode(0o700);

		fs::set_permissions(path, permissions)?;
	}

	Ok(())
}

fn sanitize_path_component(value: &str) -> String {
	let sanitized = value
		.chars()
		.map(|character| {
			if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
				character
			} else {
				'_'
			}
		})
		.collect::<String>();

	if sanitized.is_empty() { String::from("git") } else { sanitized }
}
