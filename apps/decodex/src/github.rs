mod branch;
mod command;
mod comments;
mod landing_state;
mod merge_readback;
mod repository;

#[cfg(test)]
pub(crate) use self::{
	branch::{gh_delete_ref_missing_branch, github_api_ref_path},
	command::{GH_FALLBACK_PATHS, GhCommandDiscoveryTier, gh_command_resolution_from_env},
};
pub(crate) use self::{
	command::{
		GhCommandResolution, configure_gh_command, gh_command_resolution, gh_command_with_config,
	},
	comments::post_pull_request_issue_comment,
	landing_state::inspect_pull_request_landing_state,
	merge_readback::{
		PullRequestMergeViewResponse, admin_merge_pull_request, inspect_pull_request_merge_commit,
		inspect_pull_request_merge_readback, pull_request_is_merged_at_head,
		wait_for_commit_subject, wait_for_pull_request_merge_commit,
	},
};
pub(crate) use branch::delete_pull_request_head_branch_if_present;
#[cfg(test)]
pub(crate) use merge_readback::{
	commit_subject_wait_error_is_retryable, configure_admin_merge_command,
	merge_commit_wait_error_is_retryable,
};
pub(crate) use repository::{
	RepositoryContext, inspect_repository_context, pull_request_matches_repository,
};

use crate::prelude::{Result, eyre};

#[derive(Debug)]
pub(crate) struct PullRequestLocator {
	pub(crate) owner: String,
	pub(crate) repo: String,
	pub(crate) number: u64,
}

pub(crate) fn parse_pull_request_url(pr_url: &str) -> Result<PullRequestLocator> {
	let normalized = pr_url.trim().trim_end_matches('/');
	let suffix = normalized.strip_prefix("https://github.com/").ok_or_else(|| {
		eyre::eyre!("Pull request URL `{pr_url}` must start with `https://github.com/`.")
	})?;
	let mut segments = suffix.split('/');
	let owner = segments
		.next()
		.filter(|value| !value.is_empty())
		.ok_or_else(|| eyre::eyre!("Pull request URL `{pr_url}` is missing the owner."))?;
	let repo = segments
		.next()
		.filter(|value| !value.is_empty())
		.ok_or_else(|| eyre::eyre!("Pull request URL `{pr_url}` is missing the repository."))?;
	let pull_segment = segments
		.next()
		.ok_or_else(|| eyre::eyre!("Pull request URL `{pr_url}` is missing the `pull` segment."))?;

	if pull_segment != "pull" {
		eyre::bail!(
			"Pull request URL `{pr_url}` must use `/pull/<number>`, not `/{pull_segment}`."
		);
	}

	let number = segments
		.next()
		.ok_or_else(|| {
			eyre::eyre!("Pull request URL `{pr_url}` is missing the pull request number.")
		})?
		.parse::<u64>()
		.map_err(|error| {
			eyre::eyre!("Pull request URL `{pr_url}` has an invalid number: {error}")
		})?;

	Ok(PullRequestLocator { owner: owner.to_owned(), repo: repo.to_owned(), number })
}

#[cfg(test)]
mod tests {
	use std::{
		ffi::{OsStr, OsString},
		fs,
	};

	use crate::prelude::eyre;

	#[test]
	fn parses_pull_request_url() {
		let locator = super::parse_pull_request_url("https://github.com/hack-ink/decodex/pull/20")
			.expect("pull request URL should parse");

		assert_eq!(locator.owner, "hack-ink");
		assert_eq!(locator.repo, "decodex");
		assert_eq!(locator.number, 20);
	}

	#[test]
	fn rejects_non_pull_github_url() {
		let error = super::parse_pull_request_url("https://github.com/hack-ink/decodex/issues/20")
			.expect_err("issue URL should be rejected");

		assert!(error.to_string().contains("/pull/<number>"));
	}

	#[test]
	fn rejects_missing_number() {
		let error = super::parse_pull_request_url("https://github.com/hack-ink/decodex/pull/")
			.expect_err("missing pull number should be rejected");

		assert!(error.to_string().contains("missing the pull request number"));
	}

	#[test]
	fn configure_gh_command_sets_explicit_token_when_present() {
		let mut command = std::process::Command::new("gh");

		super::configure_gh_command(&mut command, "ghp_example");

		let envs = command
			.get_envs()
			.filter_map(|(key, value)| Some((key.to_owned(), value?.to_owned())))
			.collect::<std::collections::HashMap<_, _>>();

		assert_eq!(envs.get(OsStr::new("GH_TOKEN")), Some(&OsStr::new("ghp_example").to_owned()));
		assert_eq!(
			envs.get(OsStr::new("GITHUB_TOKEN")),
			Some(&OsStr::new("ghp_example").to_owned())
		);
	}

	#[test]
	fn configure_gh_command_disables_prompt_for_explicit_token_auth() {
		let mut command = std::process::Command::new("gh");

		super::configure_gh_command(&mut command, "ghp_example");

		assert!(
			command
				.get_envs()
				.find_map(|(key, value)| (key == OsStr::new("GH_PROMPT_DISABLED")).then_some(value))
				.flatten()
				.is_some_and(|value| value == OsStr::new("1")),
			"configure_gh_command should disable interactive gh prompts"
		);
		assert!(
			command
				.get_envs()
				.find_map(|(key, value)| (key == OsStr::new("GIT_TERMINAL_PROMPT")).then_some(value))
				.flatten()
				.is_some_and(|value| value == OsStr::new("0")),
			"configure_gh_command should disable interactive git prompts"
		);
		assert!(
			command
				.get_envs()
				.find_map(|(key, value)| (key == OsStr::new("GCM_INTERACTIVE")).then_some(value))
				.flatten()
				.is_some_and(|value| value == OsStr::new("never")),
			"configure_gh_command should disable credential-manager prompts"
		);
	}

	#[test]
	fn gh_command_resolution_prefers_path_candidate() {
		let temp_dir = tempfile::TempDir::new().expect("temp dir should exist");
		let gh_path = temp_dir.path().join("gh");

		fs::write(&gh_path, "").expect("fake gh should write");

		let resolution = super::gh_command_resolution_from_env(
			None,
			Some(OsString::from(temp_dir.path().as_os_str())),
			None,
		);

		assert_eq!(resolution.command_path(), gh_path.as_path());
		assert_eq!(resolution.resolved_path(), Some(gh_path.as_path()));
		assert_eq!(resolution.discovery_tier(), super::GhCommandDiscoveryTier::Path);
	}

	#[test]
	fn gh_command_resolution_falls_back_to_home_local_bin() {
		let temp_dir = tempfile::TempDir::new().expect("temp dir should exist");
		let bin_dir = temp_dir.path().join(".local/bin");
		let gh_path = bin_dir.join("gh");

		fs::create_dir_all(&bin_dir).expect("fake home bin should exist");
		fs::write(&gh_path, "").expect("fake gh should write");

		let resolution = super::gh_command_resolution_from_env(
			None,
			Some(OsString::new()),
			Some(OsString::from(temp_dir.path().as_os_str())),
		);

		assert_eq!(resolution.command_path(), gh_path.as_path());
		assert_eq!(resolution.resolved_path(), Some(gh_path.as_path()));
		assert_eq!(resolution.discovery_tier(), super::GhCommandDiscoveryTier::UserBin);
	}

	#[test]
	fn gh_command_resolution_uses_configured_path_as_authority() {
		let temp_dir = tempfile::TempDir::new().expect("temp dir should exist");
		let gh_path = temp_dir.path().join("configured-gh");

		fs::write(&gh_path, "").expect("fake configured gh should write");

		let resolution =
			super::gh_command_resolution_from_env(Some(&gh_path), Some(OsString::new()), None);

		assert_eq!(resolution.command_path(), gh_path.as_path());
		assert_eq!(resolution.configured_path(), Some(gh_path.as_path()));
		assert_eq!(resolution.resolved_path(), Some(gh_path.as_path()));
		assert_eq!(resolution.discovery_tier(), super::GhCommandDiscoveryTier::Configured);
	}

	#[test]
	fn gh_command_resolution_knows_nix_profile_fallback() {
		assert!(super::GH_FALLBACK_PATHS.contains(&"/run/current-system/sw/bin/gh"));
	}

	#[test]
	fn merge_commit_wait_retries_only_visibility_errors() {
		assert!(super::merge_commit_wait_error_is_retryable(&eyre::eyre!(
			"Pull request `https://github.com/hack-ink/decodex/pull/1` does not expose a merge commit after merge."
		)));
		assert!(!super::merge_commit_wait_error_is_retryable(&eyre::eyre!(
			"Failed to inspect merge result for `https://github.com/hack-ink/decodex/pull/1`: HTTP 401"
		)));
	}

	#[test]
	fn commit_subject_wait_retries_only_not_found_visibility_errors() {
		assert!(super::commit_subject_wait_error_is_retryable(&eyre::eyre!(
			"Failed to inspect merge commit `abc` for `https://github.com/hack-ink/decodex/pull/1`: HTTP 404 Not Found"
		)));
		assert!(!super::commit_subject_wait_error_is_retryable(&eyre::eyre!(
			"Failed to inspect merge commit `abc` for `https://github.com/hack-ink/decodex/pull/1`: HTTP 401 Unauthorized"
		)));
	}

	#[test]
	fn repository_match_rejects_foreign_pull_request_url() {
		let repository = super::RepositoryContext {
			owner: String::from("hack-ink"),
			name: String::from("decodex"),
			default_branch: String::from("main"),
			merge_commit_allowed: true,
		};

		assert!(
			!super::pull_request_matches_repository(
				"https://github.com/other-org/other-repo/pull/9",
				&repository
			)
			.expect("foreign pull request URL should parse")
		);
	}

	#[test]
	fn repository_match_accepts_case_insensitive_pull_request_url() {
		let repository = super::RepositoryContext {
			owner: String::from("hack-ink"),
			name: String::from("decodex"),
			default_branch: String::from("main"),
			merge_commit_allowed: true,
		};

		assert!(
			super::pull_request_matches_repository(
				"https://github.com/Hack-Ink/Decodex/pull/9",
				&repository
			)
			.expect("same repository with different casing should parse")
		);
	}

	#[test]
	fn admin_merge_command_matches_reviewed_head_commit() {
		let mut command = std::process::Command::new("gh");

		super::configure_admin_merge_command(
			&mut command,
			"https://github.com/hack-ink/decodex/pull/50",
			"deadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
			None,
		);

		let args =
			command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect::<Vec<_>>();

		assert_eq!(
			args,
			vec![
				String::from("pr"),
				String::from("merge"),
				String::from("--admin"),
				String::from("--merge"),
				String::from("--match-head-commit"),
				String::from("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef"),
				String::from("--body"),
				String::from(""),
				String::from("https://github.com/hack-ink/decodex/pull/50"),
			]
		);
	}

	#[test]
	fn admin_merge_command_includes_subject_when_provided() {
		let mut command = std::process::Command::new("gh");

		super::configure_admin_merge_command(
			&mut command,
			"https://github.com/hack-ink/decodex/pull/50",
			"deadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
			Some(r#"{"schema":"decodex/commit/1","summary":"ship fix","authority":"manual"}"#),
		);

		let args =
			command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect::<Vec<_>>();

		assert_eq!(
			args,
			vec![
				String::from("pr"),
				String::from("merge"),
				String::from("--admin"),
				String::from("--merge"),
				String::from("--match-head-commit"),
				String::from("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef"),
				String::from("--subject"),
				String::from(
					r#"{"schema":"decodex/commit/1","summary":"ship fix","authority":"manual"}"#
				),
				String::from("--body"),
				String::from(""),
				String::from("https://github.com/hack-ink/decodex/pull/50"),
			]
		);
	}

	#[test]
	fn github_api_ref_path_preserves_ref_slashes_and_encodes_segments() {
		assert_eq!(super::github_api_ref_path("y/decodex XY-235"), "y/decodex%20XY-235");
	}

	#[test]
	fn missing_remote_ref_errors_are_idempotent_cleanup() {
		let output = std::process::Output {
			status: std::process::Command::new("sh")
				.args(["-c", "exit 1"])
				.status()
				.expect("status command should run"),
			stdout: Vec::new(),
			stderr: b"gh: Reference does not exist (HTTP 422)".to_vec(),
		};

		assert!(super::gh_delete_ref_missing_branch(&output));
	}

	#[test]
	fn generic_github_not_found_is_not_idempotent_cleanup() {
		let output = std::process::Output {
			status: std::process::Command::new("sh")
				.args(["-c", "exit 1"])
				.status()
				.expect("status command should run"),
			stdout: Vec::new(),
			stderr: b"gh: Not Found (HTTP 404)".to_vec(),
		};

		assert!(!super::gh_delete_ref_missing_branch(&output));
	}
}
