use std::path::Path;

use crate::prelude::eyre::Report;

pub(super) fn is_analysis_draft_path(path: &Path) -> bool {
	let normalized = normalized_path(path);

	normalized.ends_with(".analysis.json")
		&& (normalized.contains("/generated/analysis/")
			|| normalized.starts_with("generated/analysis/"))
}

pub(super) fn analysis_draft_error_lines(error: Report) -> Vec<String> {
	error
		.to_string()
		.lines()
		.map(str::trim)
		.filter(|line| !line.is_empty())
		.map(|line| line.trim_start_matches("- ").to_owned())
		.collect()
}

fn normalized_path(path: &Path) -> String {
	path.to_string_lossy().replace('\\', "/")
}
