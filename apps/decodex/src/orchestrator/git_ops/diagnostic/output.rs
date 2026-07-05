use std::{collections::BTreeSet, process::Output};

pub(in crate::orchestrator::git_ops::diagnostic) const REPO_GATE_DIAGNOSTIC_EXCERPT_LIMIT: usize =
	4_000;

pub(crate) fn repo_gate_output_text(output: &Output) -> String {
	let stderr = String::from_utf8_lossy(&output.stderr);
	let stdout = String::from_utf8_lossy(&output.stdout);
	let stderr = stderr.trim();
	let stdout = stdout.trim();

	if !stderr.is_empty() {
		return stderr.to_owned();
	}
	if !stdout.is_empty() {
		return stdout.to_owned();
	}

	String::from("(command produced no output)")
}

pub(crate) fn repo_gate_git_output_lines(output: &Output) -> BTreeSet<String> {
	let stdout = String::from_utf8_lossy(&output.stdout);

	stdout.lines().map(str::trim).filter(|line| !line.is_empty()).map(str::to_owned).collect()
}

pub(in crate::orchestrator::git_ops::diagnostic) fn repo_gate_bounded_output_excerpt(
	output_text: &str,
	limit: usize,
) -> (String, bool) {
	let mut excerpt = String::new();
	let mut truncated = false;

	for character in output_text.chars() {
		if excerpt.len() + character.len_utf8() > limit {
			truncated = true;

			break;
		}

		excerpt.push(character);
	}

	(excerpt, truncated)
}
