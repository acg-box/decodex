use std::path::Path;

use crate::{
	config::ServiceConfig,
	github::{self, GhCommandResolution},
	state::ProjectRegistration,
};

use super::OperatorGitHubCliAuthority;

pub(super) fn operator_github_cli_authority(project: &ServiceConfig) -> OperatorGitHubCliAuthority {
	operator_github_cli_authority_from_resolution(&github::gh_command_resolution(
		project.github().command_path(),
	))
}

pub(super) fn operator_github_cli_authority_from_registration(
	project: &ProjectRegistration,
) -> OperatorGitHubCliAuthority {
	let configured_path = ServiceConfig::from_path(project.config_path())
		.ok()
		.and_then(|config| config.github().command_path().map(Path::to_path_buf));

	operator_github_cli_authority_from_resolution(&github::gh_command_resolution(
		configured_path.as_deref(),
	))
}

fn operator_github_cli_authority_from_resolution(
	resolution: &GhCommandResolution,
) -> OperatorGitHubCliAuthority {
	let discovery_tier = resolution.discovery_tier().as_str().to_owned();
	let configured_path = resolution.configured_path().map(display_path);
	let available = resolution.available();

	OperatorGitHubCliAuthority {
		command_path: public_discovered_path(discovery_tier.as_str(), resolution.command_path()),
		resolved_path: resolution
			.resolved_path()
			.map(|path| public_discovered_path(discovery_tier.as_str(), path)),
		configured_path,
		discovery_tier: discovery_tier.clone(),
		available,
		next_action: github_cli_authority_next_action(discovery_tier.as_str(), available),
	}
}

fn github_cli_authority_next_action(discovery_tier: &str, available: bool) -> String {
	match (discovery_tier, available) {
		("configured", true) => {
			String::from("No action needed; Decodex will use the configured GitHub CLI path.")
		},
		("configured", false) => String::from(
			"Fix `github.command_path` in project.toml so it points to an installed `gh` binary.",
		),
		("path", true) => {
			String::from("No action needed; Decodex resolved `gh` from the process PATH.")
		},
		("user-bin" | "known-fallback", true) => String::from(
			"Set `github.command_path` in project.toml if this fallback path is unexpected.",
		),
		_ => String::from(
			"Install GitHub CLI or set `github.command_path` in project.toml to the expected `gh` binary.",
		),
	}
}

fn display_path(path: &Path) -> String {
	path.display().to_string()
}

fn public_discovered_path(discovery_tier: &str, path: &Path) -> String {
	if discovery_tier == "configured" {
		return display_path(path);
	}

	path.file_name()
		.and_then(|name| name.to_str())
		.map(str::to_owned)
		.unwrap_or_else(|| String::from("gh"))
}
