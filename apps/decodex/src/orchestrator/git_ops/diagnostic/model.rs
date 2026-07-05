use std::{fmt::Display, process::Output};

use serde_json::{self, Value};

use crate::orchestrator::git_ops::diagnostic::{
	output::{self, REPO_GATE_DIAGNOSTIC_EXCERPT_LIMIT},
	problem,
};

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
	pub(in crate::orchestrator::git_ops) fn from_output(
		stage: &'static str,
		failed_command: &str,
		output: &Output,
		output_text: &str,
	) -> Self {
		let (output_excerpt, output_truncated) = output::repo_gate_bounded_output_excerpt(
			output_text,
			REPO_GATE_DIAGNOSTIC_EXCERPT_LIMIT,
		);
		let problem_lines = problem::repo_gate_problem_lines(output_text);
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

	pub(in crate::orchestrator::git_ops) fn from_spawn_error(
		stage: &'static str,
		failed_command: &str,
		error: &dyn Display,
	) -> Self {
		let output_text = error.to_string();
		let (output_excerpt, output_truncated) = output::repo_gate_bounded_output_excerpt(
			&output_text,
			REPO_GATE_DIAGNOSTIC_EXCERPT_LIMIT,
		);
		let problem_lines = problem::repo_gate_problem_lines(&output_text);
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
