use std::{collections::BTreeSet, path::Path};

use serde_json::Value;

use crate::{orchestrator, state};

pub(crate) fn phase_acceptance_changed_surfaces(worktree_path: &Path) -> Vec<String> {
	let mut surfaces = BTreeSet::new();

	if let Ok(changed_files) = orchestrator::repo_gate_changed_tracked_files(worktree_path) {
		surfaces.extend(changed_files);
	}
	if let Ok(Some(diff_paths)) = orchestrator::git_guardrail_output(
		worktree_path,
		&["diff", "--name-only", "--diff-filter=ACDMRTUXB", "HEAD", "--"],
	) {
		for path in diff_paths.lines().map(str::trim).filter(|path| !path.is_empty()) {
			surfaces.insert(path.to_owned());
		}
	}
	if let Ok(Some(status)) =
		orchestrator::git_guardrail_output(worktree_path, &["status", "--porcelain"])
	{
		for surface in status.lines().filter_map(phase_acceptance_status_surface) {
			surfaces.insert(surface);
		}
	}

	surfaces.into_iter().collect()
}

pub(crate) fn phase_acceptance_blocker_count(payload: &Value) -> usize {
	payload.get("blockers").and_then(Value::as_array).map_or(0, Vec::len)
}

pub(crate) fn phase_acceptance_docs_impact_valid(value: &str) -> bool {
	matches!(value, "none" | "update_required" | "research_required" | "drift_required")
}

pub(crate) fn phase_acceptance_has_non_goal_violation(payload: &Value) -> bool {
	payload
		.get("blockers")
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
		.filter_map(Value::as_str)
		.any(|blocker| {
			let normalized = blocker.to_ascii_lowercase();

			normalized.contains("non-goal")
				|| normalized.contains("non_goal")
				|| normalized.contains("out of scope")
				|| normalized.contains("scope violation")
		})
}

fn phase_acceptance_status_surface(line: &str) -> Option<String> {
	let line = line.trim_end();

	if line.is_empty() || state::is_untracked_decodex_runtime_artifact_status_line(line) {
		return None;
	}

	let path = line.get(3..)?.trim();
	let path = path.rsplit_once(" -> ").map_or(path, |(_, renamed_path)| renamed_path.trim());

	(!path.is_empty()).then(|| path.to_owned())
}
