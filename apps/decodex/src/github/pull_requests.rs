use std::path::Path;

use crate::{
	github::{self},
	prelude::{Result, eyre},
};

pub(crate) fn close_pull_request(
	cwd: &Path,
	pr_url: &str,
	github_token: &str,
	gh_command_path: Option<&Path>,
) -> Result<()> {
	let locator = github::parse_pull_request_url(pr_url)?;
	let endpoint = format!("repos/{}/{}/pulls/{}", locator.owner, locator.repo, locator.number);
	let mut command = github::gh_command_with_config(gh_command_path);

	command.args(["api", "--method", "PATCH", endpoint.as_str(), "-f", "state=closed"]);
	command.current_dir(cwd);

	github::configure_gh_command(&mut command, github_token);

	let output = command.output()?;

	if output.status.success() {
		return Ok(());
	}

	let stderr = String::from_utf8_lossy(&output.stderr);

	eyre::bail!("Failed to close pull request `{pr_url}`: {}", stderr.trim())
}

#[cfg(test)]
mod tests {
	use std::{fs, os::unix::fs::PermissionsExt};

	use tempfile::TempDir;

	use super::*;

	#[test]
	fn close_pull_request_patches_pull_request_state() {
		let temp_dir = TempDir::new().expect("temp dir should create");
		let gh_path = temp_dir.path().join("gh");
		let log_path = temp_dir.path().join("gh.log");
		let script = format!(
			r#"#!/bin/sh
printf '%s\n' "$*" > '{}'
printf '{{"state":"closed"}}'
"#,
			log_path.display()
		);

		fs::write(&gh_path, script).expect("fake gh should write");
		let mut permissions = fs::metadata(&gh_path).expect("fake gh metadata").permissions();
		permissions.set_mode(0o755);
		fs::set_permissions(&gh_path, permissions).expect("fake gh should be executable");

		close_pull_request(
			temp_dir.path(),
			"https://github.com/helixbox/pubfi-mono/pull/826",
			"ghp_test",
			Some(&gh_path),
		)
		.expect("close should succeed");

		let log = fs::read_to_string(log_path).expect("fake gh should log args");

		assert!(log.contains("api --method PATCH repos/helixbox/pubfi-mono/pulls/826"));
		assert!(log.contains("-f state=closed"));
	}
}
