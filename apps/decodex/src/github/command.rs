use std::{
	env,
	ffi::OsString,
	path::{Path, PathBuf},
	process::Command,
};

use crate::git_credentials;

const GH_BINARY: &str = "gh";
pub(crate) const GH_FALLBACK_PATHS: &[&str] =
	&["/run/current-system/sw/bin/gh", "/opt/homebrew/bin/gh", "/usr/local/bin/gh", "/usr/bin/gh"];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GhCommandDiscoveryTier {
	Configured,
	Path,
	UserBin,
	KnownFallback,
	Missing,
}
impl GhCommandDiscoveryTier {
	pub(crate) const fn as_str(self) -> &'static str {
		match self {
			Self::Configured => "configured",
			Self::Path => "path",
			Self::UserBin => "user-bin",
			Self::KnownFallback => "known-fallback",
			Self::Missing => "missing",
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GhCommandResolution {
	command_path: PathBuf,
	resolved_path: Option<PathBuf>,
	configured_path: Option<PathBuf>,
	discovery_tier: GhCommandDiscoveryTier,
}
impl GhCommandResolution {
	pub(crate) fn command_path(&self) -> &Path {
		&self.command_path
	}

	pub(crate) fn resolved_path(&self) -> Option<&Path> {
		self.resolved_path.as_deref()
	}

	pub(crate) fn configured_path(&self) -> Option<&Path> {
		self.configured_path.as_deref()
	}

	pub(crate) const fn discovery_tier(&self) -> GhCommandDiscoveryTier {
		self.discovery_tier
	}

	pub(crate) const fn available(&self) -> bool {
		self.resolved_path.is_some()
	}
}

pub(crate) fn configure_gh_command(command: &mut Command, github_token: &str) {
	git_credentials::clear_injected_git_config(command);

	command
		.env("GH_TOKEN", github_token)
		.env("GITHUB_TOKEN", github_token)
		.env("GH_PROMPT_DISABLED", "1")
		.env("GIT_TERMINAL_PROMPT", "0")
		.env("GCM_INTERACTIVE", "never");
}

pub(crate) fn gh_command_with_config(configured_path: Option<&Path>) -> Command {
	Command::new(gh_command_resolution(configured_path).command_path())
}

pub(crate) fn gh_command_resolution(configured_path: Option<&Path>) -> GhCommandResolution {
	gh_command_resolution_from_env(configured_path, env::var_os("PATH"), env::var_os("HOME"))
}

pub(crate) fn gh_command_resolution_from_env(
	configured_path: Option<&Path>,
	path_env: Option<OsString>,
	home: Option<OsString>,
) -> GhCommandResolution {
	if let Some(configured_path) = configured_path {
		let command_path = configured_path.to_path_buf();
		let resolved_path = command_path.is_file().then_some(command_path.clone());

		return GhCommandResolution {
			command_path,
			resolved_path,
			configured_path: Some(configured_path.to_path_buf()),
			discovery_tier: GhCommandDiscoveryTier::Configured,
		};
	}
	if let Some(path_env) = path_env {
		for path_entry in env::split_paths(&path_env) {
			let candidate = path_entry.join(GH_BINARY);

			if candidate.is_file() {
				return GhCommandResolution {
					command_path: candidate.clone(),
					resolved_path: Some(candidate),
					configured_path: None,
					discovery_tier: GhCommandDiscoveryTier::Path,
				};
			}
		}
	}
	if let Some(home) = home {
		let home = PathBuf::from(home);

		for relative_candidate in [[".local", "bin", GH_BINARY], [".cargo", "bin", GH_BINARY]] {
			let candidate = relative_candidate
				.iter()
				.fold(home.clone(), |path, component| path.join(*component));

			if candidate.is_file() {
				return GhCommandResolution {
					command_path: candidate.clone(),
					resolved_path: Some(candidate),
					configured_path: None,
					discovery_tier: GhCommandDiscoveryTier::UserBin,
				};
			}
		}
	}

	for candidate in GH_FALLBACK_PATHS {
		let candidate = PathBuf::from(candidate);

		if candidate.is_file() {
			return GhCommandResolution {
				command_path: candidate.clone(),
				resolved_path: Some(candidate),
				configured_path: None,
				discovery_tier: GhCommandDiscoveryTier::KnownFallback,
			};
		}
	}

	GhCommandResolution {
		command_path: PathBuf::from(GH_BINARY),
		resolved_path: None,
		configured_path: None,
		discovery_tier: GhCommandDiscoveryTier::Missing,
	}
}
