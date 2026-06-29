use std::{path::Path, process::Command};

use crate::config::ServiceConfig;

pub(super) fn operator_project_display_name(project: &ServiceConfig) -> String {
	github_repo_slug_from_origin(project.repo_root())
		.or_else(|| repo_root_path_display_name(project.repo_root()))
		.unwrap_or_else(|| project.service_id().to_owned())
}

fn github_repo_slug_from_origin(repo_root: &Path) -> Option<String> {
	let output = Command::new("git")
		.arg("-C")
		.arg(repo_root)
		.args(["config", "--get", "remote.origin.url"])
		.output()
		.ok()?;

	if !output.status.success() {
		return None;
	}

	let remote_url = String::from_utf8(output.stdout).ok()?;

	parse_github_remote_slug(remote_url.trim())
}

fn parse_github_remote_slug(remote_url: &str) -> Option<String> {
	let path = remote_url
		.strip_prefix("git@github.com:")
		.or_else(|| remote_url.strip_prefix("git@github.com-x:"))
		.or_else(|| remote_url.strip_prefix("git@github.com-y:"))
		.or_else(|| github_remote_path_with_authority(remote_url))?;
	let path = path.trim_start_matches('/').trim_end_matches(".git");
	let mut components = path.split('/').filter(|component| !component.trim().is_empty());
	let owner = components.next()?.trim();
	let repo = components.next()?.trim();

	if components.next().is_some() {
		return None;
	}

	Some(format!("{owner}/{repo}"))
}

fn github_remote_path_with_authority(remote_url: &str) -> Option<&str> {
	let rest = remote_url
		.strip_prefix("https://")
		.or_else(|| remote_url.strip_prefix("http://"))
		.or_else(|| remote_url.strip_prefix("ssh://"))?;
	let (authority, path) = rest.split_once('/')?;
	let host = authority.rsplit('@').next().unwrap_or(authority);
	let host = host.split(':').next().unwrap_or(host);

	if !matches!(host, "github.com" | "github.com-x" | "github.com-y") {
		return None;
	}

	Some(path)
}

fn repo_root_path_display_name(repo_root: &Path) -> Option<String> {
	let repo = repo_root.file_name()?.to_string_lossy();
	let repo = repo.trim();

	if repo.is_empty() {
		return None;
	}

	let Some(parent) = repo_root.parent().and_then(Path::file_name) else {
		return Some(repo.to_owned());
	};
	let parent = parent.to_string_lossy();
	let parent = parent.trim();

	if parent.is_empty() {
		return Some(repo.to_owned());
	}

	Some(format!("{parent}/{repo}"))
}
