use std::{collections::BTreeSet, fmt::Display, process::Output};

use serde_json::{self, Value};

use crate::orchestrator::git_ops::RepoGateFailureKind;

const REPO_GATE_DIAGNOSTIC_EXCERPT_LIMIT: usize = 4_000;
const REPO_GATE_DIAGNOSTIC_LINE_LIMIT: usize = 16;
const REPO_GATE_DIAGNOSTIC_LINE_WIDTH: usize = 320;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RepoGateFailureDiagnostic {
	stage: &'static str,
	failed_command: String,
	exit_status: Option<i32>,
	summary: String,
	problem_lines: Vec<String>,
	output_excerpt: String,
	output_truncated: bool,
}
impl RepoGateFailureDiagnostic {
	pub(super) fn from_output(
		stage: &'static str,
		failed_command: &str,
		output: &Output,
		output_text: &str,
	) -> Self {
		let (output_excerpt, output_truncated) =
			repo_gate_bounded_output_excerpt(output_text, REPO_GATE_DIAGNOSTIC_EXCERPT_LIMIT);
		let problem_lines = repo_gate_problem_lines(output_text);
		let summary = repo_gate_diagnostic_summary(stage, failed_command, output, &problem_lines);

		Self {
			stage,
			failed_command: failed_command.to_owned(),
			exit_status: output.status.code(),
			summary,
			problem_lines,
			output_excerpt,
			output_truncated,
		}
	}

	pub(super) fn from_spawn_error(
		stage: &'static str,
		failed_command: &str,
		error: &dyn Display,
	) -> Self {
		let output_text = error.to_string();
		let (output_excerpt, output_truncated) =
			repo_gate_bounded_output_excerpt(&output_text, REPO_GATE_DIAGNOSTIC_EXCERPT_LIMIT);
		let problem_lines = repo_gate_problem_lines(&output_text);
		let summary = format!("Repo gate {stage} command `{failed_command}` failed to spawn.");

		Self {
			stage,
			failed_command: failed_command.to_owned(),
			exit_status: None,
			summary,
			problem_lines,
			output_excerpt,
			output_truncated,
		}
	}

	pub(crate) fn repair_target_detail(&self) -> String {
		let key_lines = if self.problem_lines.is_empty() {
			String::from("none")
		} else {
			self.problem_lines.join(" | ")
		};

		format!(
			"Failed repo-gate command: `{}` during `{}`. Summary: {} Key diagnostic lines: {}.",
			self.failed_command, self.stage, self.summary, key_lines
		)
	}

	pub(crate) fn to_json(&self) -> Value {
		serde_json::json!({
			"schema": "decodex.repo_gate_failure_diagnostic/1",
			"stage": self.stage,
			"failed_command": &self.failed_command,
			"exit_status": self.exit_status,
			"summary": &self.summary,
			"problem_lines": &self.problem_lines,
			"output_excerpt": &self.output_excerpt,
			"output_truncated": self.output_truncated,
		})
	}
}

impl RepoGateFailureKind {
	pub(super) fn retry_schedule_kind(self) -> Option<&'static str> {
		match self {
			Self::GitLockContention => Some("git_lock_contention"),
			_ => None,
		}
	}
}

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

pub(super) fn repo_gate_git_output_lines(output: &Output) -> BTreeSet<String> {
	let stdout = String::from_utf8_lossy(&output.stdout);

	stdout.lines().map(str::trim).filter(|line| !line.is_empty()).map(str::to_owned).collect()
}

pub(super) fn repo_gate_failure_kind_for_output(
	default_kind: RepoGateFailureKind,
	output_text: &str,
) -> RepoGateFailureKind {
	if repo_gate_is_git_lock_contention(output_text) {
		RepoGateFailureKind::GitLockContention
	} else {
		default_kind
	}
}

fn repo_gate_bounded_output_excerpt(output_text: &str, limit: usize) -> (String, bool) {
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

fn repo_gate_diagnostic_summary(
	stage: &str,
	failed_command: &str,
	output: &Output,
	problem_lines: &[String],
) -> String {
	let exit_status =
		output.status.code().map_or_else(|| String::from("unknown"), |code| code.to_string());
	let first_problem = problem_lines
		.first()
		.map_or_else(|| String::from("no diagnostic output"), ToOwned::to_owned);

	format!(
		"repo gate {stage} command `{failed_command}` exited with status {exit_status}: {first_problem}"
	)
}

fn repo_gate_problem_lines(output_text: &str) -> Vec<String> {
	let lines = output_text.lines().collect::<Vec<_>>();
	let mut selected_indexes = BTreeSet::new();

	for (index, line) in lines.iter().enumerate() {
		if repo_gate_line_looks_diagnostic(line) {
			selected_indexes.insert(index);

			if index > 0 {
				selected_indexes.insert(index - 1);
			}

			for follow_index in index.saturating_add(1)..=(index + 4).min(lines.len()) {
				selected_indexes.insert(follow_index);
			}
		}
	}

	if selected_indexes.is_empty() {
		selected_indexes.extend(
			lines
				.iter()
				.enumerate()
				.filter(|(_, line)| !line.trim().is_empty())
				.take(4)
				.map(|(index, _)| index),
		);
	}

	selected_indexes
		.into_iter()
		.filter_map(|index| lines.get(index))
		.map(|line| repo_gate_truncate_diagnostic_line(line.trim()))
		.filter(|line| !line.is_empty())
		.take(REPO_GATE_DIAGNOSTIC_LINE_LIMIT)
		.collect()
}

fn repo_gate_line_looks_diagnostic(line: &str) -> bool {
	let line = line.trim();
	let lower = line.to_ascii_lowercase();

	line.starts_with("-->")
		|| lower.starts_with("error")
		|| lower.starts_with("fatal")
		|| lower.starts_with("failed")
		|| lower.starts_with("warning")
		|| lower.contains(" error:")
		|| lower.contains(" fatal:")
		|| lower.contains(" failed")
		|| lower.contains("panicked at")
		|| lower.contains("too many lines")
		|| lower.contains("clippy")
}

fn repo_gate_truncate_diagnostic_line(line: &str) -> String {
	let (mut line, truncated) =
		repo_gate_bounded_output_excerpt(line, REPO_GATE_DIAGNOSTIC_LINE_WIDTH);

	if truncated {
		line.push_str("...");
	}

	line
}

fn repo_gate_is_git_lock_contention(output_text: &str) -> bool {
	let output_text = output_text.to_ascii_lowercase();

	output_text.contains("index.lock")
		&& (output_text.contains("file exists")
			|| output_text.contains("already exists")
			|| output_text.contains("another git process seems to be running"))
}
