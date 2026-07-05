use std::collections::BTreeSet;

use crate::orchestrator::git_ops::diagnostic::output;

const REPO_GATE_DIAGNOSTIC_LINE_LIMIT: usize = 16;
const REPO_GATE_DIAGNOSTIC_LINE_WIDTH: usize = 320;

pub(in crate::orchestrator::git_ops::diagnostic) fn repo_gate_problem_lines(
	output_text: &str,
) -> Vec<String> {
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
		output::repo_gate_bounded_output_excerpt(line, REPO_GATE_DIAGNOSTIC_LINE_WIDTH);

	if truncated {
		line.push_str("...");
	}

	line
}
