use std::{path::Path, process::Command};

pub(in crate::worktree) fn configured_branch_owner(repo_root: &Path) -> Option<String> {
	let output = Command::new("git")
		.arg("-C")
		.arg(repo_root)
		.args(["config", "--get", "codex.github-identity"])
		.output()
		.ok()?;

	if !output.status.success() {
		return None;
	}

	let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();

	(!value.is_empty()).then_some(value)
}

pub(in crate::worktree) fn sanitize_branch_component(value: &str) -> String {
	value
		.chars()
		.map(|ch| match ch {
			'A'..='Z' => ch.to_ascii_lowercase(),
			'a'..='z' | '0'..='9' => ch,
			'-' | '_' => '-',
			_ => '-',
		})
		.collect::<String>()
		.trim_matches('-')
		.to_owned()
}
