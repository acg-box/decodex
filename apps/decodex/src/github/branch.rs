use std::{path::Path, process::Output};

use crate::{
	github::{self},
	prelude::{Result, eyre},
};

pub(crate) fn delete_pull_request_head_branch_if_present(
	cwd: &Path,
	pr_url: &str,
	branch_name: &str,
	github_token: &str,
	gh_command_path: Option<&Path>,
) -> Result<()> {
	let locator = github::parse_pull_request_url(pr_url)?;

	delete_repository_branch_if_present(
		cwd,
		&locator.owner,
		&locator.repo,
		branch_name,
		github_token,
		gh_command_path,
	)
}

pub(crate) fn gh_delete_ref_missing_branch(output: &Output) -> bool {
	let stderr = String::from_utf8_lossy(&output.stderr).to_ascii_lowercase();
	let stdout = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
	let combined = format!("{stderr}\n{stdout}");

	combined.contains("reference does not exist")
		|| combined.contains("reference not found")
		|| (combined.contains("http 422") && combined.contains("reference"))
}

pub(crate) fn github_api_ref_path(ref_name: &str) -> String {
	ref_name.split('/').map(github_api_path_component).collect::<Vec<_>>().join("/")
}

fn delete_repository_branch_if_present(
	cwd: &Path,
	owner: &str,
	repo: &str,
	branch_name: &str,
	github_token: &str,
	gh_command_path: Option<&Path>,
) -> Result<()> {
	if branch_name.trim().is_empty() {
		eyre::bail!("Refusing to delete an empty GitHub branch name.");
	}

	let endpoint =
		format!("repos/{owner}/{repo}/git/refs/heads/{}", github_api_ref_path(branch_name));
	let mut command = github::gh_command_with_config(gh_command_path);

	command.args(["api", "--method", "DELETE", "--silent", endpoint.as_str()]);
	command.current_dir(cwd);

	github::configure_gh_command(&mut command, github_token);

	let output = command.output()?;

	if output.status.success() || gh_delete_ref_missing_branch(&output) {
		return Ok(());
	}

	let stderr = String::from_utf8_lossy(&output.stderr);
	let stdout = String::from_utf8_lossy(&output.stdout);
	let detail = if stderr.trim().is_empty() { stdout.trim() } else { stderr.trim() };

	eyre::bail!(
		"Failed to delete retained remote branch `{branch_name}` from GitHub repository `{owner}/{repo}`: {detail}"
	);
}

fn github_api_path_component(component: &str) -> String {
	let mut encoded = String::new();

	for byte in component.bytes() {
		if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
			encoded.push(char::from(byte));
		} else {
			encoded.push_str(&format!("%{byte:02X}"));
		}
	}

	encoded
}
