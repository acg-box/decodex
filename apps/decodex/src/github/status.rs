use std::path::Path;

use serde::Deserialize;

use crate::{
	github,
	prelude::{Result, eyre},
	pull_request::PullRequestRequiredStatusContext,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CommitStatusState {
	Error,
	Failure,
	Pending,
	Success,
}
impl CommitStatusState {
	pub(crate) const fn as_str(self) -> &'static str {
		match self {
			Self::Error => "error",
			Self::Failure => "failure",
			Self::Pending => "pending",
			Self::Success => "success",
		}
	}
}

pub(crate) struct CommitStatusPublishRequest<'a> {
	pub(crate) cwd: &'a Path,
	pub(crate) owner: &'a str,
	pub(crate) repo: &'a str,
	pub(crate) sha: &'a str,
	pub(crate) context: &'a str,
	pub(crate) state: CommitStatusState,
	pub(crate) description: Option<&'a str>,
	pub(crate) target_url: Option<&'a str>,
	pub(crate) github_token: &'a str,
	pub(crate) gh_command_path: Option<&'a Path>,
}

#[derive(Debug, Deserialize)]
struct CommitStatusResponse {
	context: String,
	state: String,
	description: Option<String>,
	creator: Option<CommitStatusCreator>,
	updated_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CommitStatusCreator {
	login: String,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn inspect_required_commit_status_contexts(
	cwd: &Path,
	owner: &str,
	repo: &str,
	sha: &str,
	current_base_ref_oid: Option<&str>,
	required_contexts: &[String],
	allowed_creators: &[String],
	github_token: &str,
	gh_command_path: Option<&Path>,
) -> Result<Vec<PullRequestRequiredStatusContext>> {
	if required_contexts.is_empty() {
		return Ok(Vec::new());
	}

	let statuses = query_commit_statuses(cwd, owner, repo, sha, github_token, gh_command_path)?;

	Ok(required_contexts
		.iter()
		.map(|context| {
			let status = latest_status_for_context(&statuses, context);
			let creator_login = status
				.and_then(|status| status.creator.as_ref())
				.map(|creator| creator.login.clone());
			let base_ref_oid = status.and_then(|status| {
				status
					.description
					.as_deref()
					.and_then(commit_status_description_base_ref_oid)
					.map(str::to_owned)
			});
			let base_ref_matches = current_base_ref_oid
				.zip(base_ref_oid.as_deref())
				.is_some_and(|(current, published)| current == published);
			let allowed_creator = allowed_creators.is_empty()
				|| creator_login
					.as_deref()
					.is_some_and(|login| allowed_creators.iter().any(|allowed| allowed == login));

			PullRequestRequiredStatusContext {
				context: context.clone(),
				state: status.map(|status| status.state.clone()),
				creator_login,
				allowed_creator,
				base_ref_oid,
				base_ref_matches,
			}
		})
		.collect())
}

fn latest_status_for_context<'a>(
	statuses: &'a [CommitStatusResponse],
	context: &str,
) -> Option<&'a CommitStatusResponse> {
	statuses
		.iter()
		.filter(|status| status.context == context)
		.max_by_key(|status| status.updated_at.as_deref().unwrap_or(""))
}

pub(crate) fn commit_status_description_with_base_ref_oid(
	description: Option<&str>,
	base_ref_oid: &str,
) -> String {
	match description.map(str::trim).filter(|value| !value.is_empty()) {
		Some(description) => format!("{description}; base_ref_oid={base_ref_oid}"),
		None => format!("base_ref_oid={base_ref_oid}"),
	}
}

fn commit_status_description_base_ref_oid(description: &str) -> Option<&str> {
	description
		.split(|character: char| character.is_whitespace() || character == ';' || character == ',')
		.find_map(|part| part.strip_prefix("base_ref_oid=").filter(|value| !value.is_empty()))
}

pub(crate) fn publish_commit_status(request: CommitStatusPublishRequest<'_>) -> Result<()> {
	let mut command = github::gh_command_with_config(request.gh_command_path);
	let endpoint = format!("repos/{}/{}/statuses/{}", request.owner, request.repo, request.sha);

	command.args(["api", "--method", "POST", &endpoint]);
	command.args(["-f", &format!("state={}", request.state.as_str())]);
	command.args(["-f", &format!("context={}", request.context)]);
	if let Some(description) = request.description {
		command.args(["-f", &format!("description={description}")]);
	}
	if let Some(target_url) = request.target_url {
		command.args(["-f", &format!("target_url={target_url}")]);
	}
	command.current_dir(request.cwd);

	github::configure_gh_command(&mut command, request.github_token);

	let output = command.output()?;
	if !output.status.success() {
		let stderr = String::from_utf8_lossy(&output.stderr);

		eyre::bail!(
			"Failed to publish commit status `{}` for `{}/{}` at `{}`: {}",
			request.context,
			request.owner,
			request.repo,
			request.sha,
			stderr.trim()
		);
	}

	Ok(())
}

fn query_commit_statuses(
	cwd: &Path,
	owner: &str,
	repo: &str,
	sha: &str,
	github_token: &str,
	gh_command_path: Option<&Path>,
) -> Result<Vec<CommitStatusResponse>> {
	let mut command = github::gh_command_with_config(gh_command_path);
	let endpoint = format!("repos/{owner}/{repo}/commits/{sha}/statuses");

	command.args(["api", &endpoint]);
	command.current_dir(cwd);

	github::configure_gh_command(&mut command, github_token);

	let output = command.output()?;
	if !output.status.success() {
		let stderr = String::from_utf8_lossy(&output.stderr);

		eyre::bail!(
			"Failed to inspect commit statuses for `{owner}/{repo}` at `{sha}`: {}",
			stderr.trim()
		);
	}

	Ok(serde_json::from_slice::<Vec<CommitStatusResponse>>(&output.stdout)?)
}

#[cfg(test)]
mod tests {
	use std::{fs, os::unix::fs::PermissionsExt};

	use tempfile::TempDir;

	use super::*;

	#[test]
	fn inspect_required_commit_status_contexts_reads_exact_context_and_creator() {
		let temp_dir = TempDir::new().expect("temp dir should exist");
		let gh_path = fake_gh(
			&temp_dir,
			r#"[{"context":"decodex/local-full-check","state":"success","creator":{"login":"decodex-bot"}},{"context":"ci/slow","state":"pending","creator":{"login":"github-actions"}}]"#,
		);

		let statuses = inspect_required_commit_status_contexts(
			temp_dir.path(),
			"hack-ink",
			"decodex",
			"head-sha",
			Some("base-sha"),
			&[String::from("decodex/local-full-check"), String::from("missing")],
			&[String::from("decodex-bot")],
			"ghp_test",
			Some(&gh_path),
		)
		.expect("status read should succeed");

		assert_eq!(statuses.len(), 2);
		assert_eq!(statuses[0].context, "decodex/local-full-check");
		assert_eq!(statuses[0].state.as_deref(), Some("success"));
		assert_eq!(statuses[0].creator_login.as_deref(), Some("decodex-bot"));
		assert!(statuses[0].allowed_creator);
		assert_eq!(statuses[0].base_ref_oid, None);
		assert!(!statuses[0].base_ref_matches);
		assert_eq!(statuses[1].context, "missing");
		assert_eq!(statuses[1].state, None);
		assert_eq!(statuses[1].creator_login, None);
		assert!(!statuses[1].allowed_creator);
		assert_eq!(statuses[1].base_ref_oid, None);
		assert!(!statuses[1].base_ref_matches);
	}

	#[test]
	fn inspect_required_commit_status_contexts_requires_current_base_oid_receipt() {
		let temp_dir = TempDir::new().expect("temp dir should exist");
		let gh_path = fake_gh(
			&temp_dir,
			r#"[{"context":"decodex/local-full-check","state":"success","description":"cargo make check passed; base_ref_oid=base-sha","creator":{"login":"decodex-bot"}}]"#,
		);

		let statuses = inspect_required_commit_status_contexts(
			temp_dir.path(),
			"hack-ink",
			"decodex",
			"head-sha",
			Some("base-sha"),
			&[String::from("decodex/local-full-check")],
			&[String::from("decodex-bot")],
			"ghp_test",
			Some(&gh_path),
		)
		.expect("status read should succeed");

		assert_eq!(statuses[0].base_ref_oid.as_deref(), Some("base-sha"));
		assert!(statuses[0].base_ref_matches);
	}

	#[test]
	fn inspect_required_commit_status_contexts_uses_latest_context_status() {
		let temp_dir = TempDir::new().expect("temp dir should exist");
		let gh_path = fake_gh(
			&temp_dir,
			r#"[{"context":"decodex/local-full-check","state":"failure","updated_at":"2026-07-09T01:00:00Z","creator":{"login":"decodex-bot"}},{"context":"decodex/local-full-check","state":"success","updated_at":"2026-07-09T02:00:00Z","creator":{"login":"yvette-carlisle"}}]"#,
		);

		let statuses = inspect_required_commit_status_contexts(
			temp_dir.path(),
			"hack-ink",
			"decodex",
			"head-sha",
			Some("base-sha"),
			&[String::from("decodex/local-full-check")],
			&[String::from("yvette-carlisle")],
			"ghp_test",
			Some(&gh_path),
		)
		.expect("status read should succeed");

		assert_eq!(statuses[0].state.as_deref(), Some("success"));
		assert_eq!(statuses[0].creator_login.as_deref(), Some("yvette-carlisle"));
		assert!(statuses[0].allowed_creator);
	}

	#[test]
	fn publish_commit_status_posts_context_to_exact_sha() {
		let temp_dir = TempDir::new().expect("temp dir should exist");
		let gh_path = fake_gh(&temp_dir, "{}");
		let log_path = temp_dir.path().join("gh.log");

		publish_commit_status(CommitStatusPublishRequest {
			cwd: temp_dir.path(),
			owner: "hack-ink",
			repo: "decodex",
			sha: "head-sha",
			context: "decodex/local-full-check",
			state: CommitStatusState::Success,
			description: Some("cargo make check passed"),
			target_url: Some("https://github.com/hack-ink/decodex/pull/42"),
			github_token: "ghp_test",
			gh_command_path: Some(&gh_path),
		})
		.expect("status publish should succeed");

		let log = fs::read_to_string(log_path).expect("fake gh should log args");

		assert!(log.contains("api --method POST repos/hack-ink/decodex/statuses/head-sha"));
		assert!(log.contains("-f state=success"));
		assert!(log.contains("-f context=decodex/local-full-check"));
		assert!(log.contains("-f description=cargo make check passed"));
	}

	fn fake_gh(temp_dir: &TempDir, response: &str) -> std::path::PathBuf {
		let gh_path = temp_dir.path().join("gh");
		let log_path = temp_dir.path().join("gh.log");
		let script = format!(
			r#"#!/bin/sh
printf '%s\n' "$*" > '{}'
cat <<'JSON'
{}
JSON
"#,
			log_path.display(),
			response
		);

		fs::write(&gh_path, script).expect("fake gh should write");
		let mut permissions = fs::metadata(&gh_path).expect("fake gh metadata").permissions();

		permissions.set_mode(0o755);
		fs::set_permissions(&gh_path, permissions).expect("fake gh should be executable");

		gh_path
	}
}
