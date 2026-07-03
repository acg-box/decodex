//! Issue identifier parsing and selector helpers for ghost-lane recovery.

use std::collections::BTreeSet;

use crate::{commit_message, state::ProjectRunStatus};

pub(super) fn ghost_lane_issue_identifier(
	run: &ProjectRunStatus,
	requested_selector: Option<&str>,
) -> Option<String> {
	requested_selector
		.filter(|selector| commit_message::looks_like_issue_identifier(selector))
		.map(str::to_ascii_uppercase)
		.or_else(|| ghost_lane_issue_identifier_from_run_id(run.run_id()))
		.or_else(|| run.branch_name().and_then(ghost_lane_issue_identifier_in_text))
		.or_else(|| {
			run.worktree_path()
				.and_then(|path| ghost_lane_issue_identifier_in_text(&path.display().to_string()))
		})
		.or_else(|| {
			commit_message::looks_like_issue_identifier(run.issue_id())
				.then(|| run.issue_id().to_ascii_uppercase())
		})
}

pub(super) fn ghost_lane_tracker_issue_selectors(
	run: &ProjectRunStatus,
	issue_identifier: Option<&str>,
	requested_selector: Option<&str>,
) -> Vec<String> {
	let mut selectors = Vec::new();

	if let Some(selector) =
		requested_selector.filter(|selector| commit_message::looks_like_issue_identifier(selector))
	{
		selectors.push(selector.to_ascii_uppercase());
	}
	if let Some(issue_identifier) = issue_identifier {
		selectors.push(issue_identifier.to_ascii_uppercase());
	}
	if let Some(inferred) = ghost_lane_inferred_issue_identifier(run) {
		selectors.push(inferred);
	}

	if commit_message::looks_like_issue_identifier(run.issue_id()) {
		selectors.push(run.issue_id().to_ascii_uppercase());
	}

	sorted_unique(selectors)
}

pub(super) fn ghost_lane_worktree_selectors(
	run: &ProjectRunStatus,
	issue_identifier: Option<&str>,
	requested_selector: Option<&str>,
) -> Vec<String> {
	let mut selectors = Vec::new();

	if let Some(selector) =
		requested_selector.filter(|selector| commit_message::looks_like_issue_identifier(selector))
	{
		selectors.push(selector.to_ascii_uppercase());
	}
	if let Some(issue_identifier) = issue_identifier {
		selectors.push(issue_identifier.to_ascii_uppercase());
	}
	if let Some(inferred) = ghost_lane_inferred_issue_identifier(run) {
		selectors.push(inferred);
	}

	if commit_message::looks_like_issue_identifier(run.issue_id()) {
		selectors.push(run.issue_id().to_ascii_uppercase());
	}

	sorted_unique(selectors)
}

pub(super) fn ghost_lane_run_matches_selector(run: &ProjectRunStatus, selector: &str) -> bool {
	if selector.eq_ignore_ascii_case(run.issue_id()) || selector.eq_ignore_ascii_case(run.run_id())
	{
		return true;
	}

	ghost_lane_worktree_selectors(run, ghost_lane_inferred_issue_identifier(run).as_deref(), None)
		.iter()
		.any(|candidate| {
			selector.eq_ignore_ascii_case(candidate)
				|| ghost_lane_identifier_suffix_matches(selector, candidate)
		})
}

fn ghost_lane_inferred_issue_identifier(run: &ProjectRunStatus) -> Option<String> {
	ghost_lane_issue_identifier_from_run_id(run.run_id())
		.or_else(|| run.branch_name().and_then(ghost_lane_issue_identifier_in_text))
		.or_else(|| {
			run.worktree_path()
				.and_then(|path| ghost_lane_issue_identifier_in_text(&path.display().to_string()))
		})
		.or_else(|| {
			commit_message::looks_like_issue_identifier(run.issue_id())
				.then(|| run.issue_id().to_ascii_uppercase())
		})
}

fn ghost_lane_issue_identifier_from_run_id(run_id: &str) -> Option<String> {
	if let Some((candidate, _attempt_suffix)) = run_id.split_once("-attempt-") {
		return ghost_lane_issue_identifier_in_text(candidate);
	}
	if let Some(candidate) = run_id.strip_prefix("recovered-") {
		return ghost_lane_issue_identifier_in_text(candidate);
	}

	None
}

fn ghost_lane_issue_identifier_in_text(value: &str) -> Option<String> {
	let bytes = value.as_bytes();

	for index in 0..bytes.len() {
		if !bytes[index].is_ascii_alphabetic() {
			continue;
		}

		let mut prefix_end = index + 1;

		while prefix_end < bytes.len() && bytes[prefix_end].is_ascii_alphanumeric() {
			prefix_end += 1;
		}

		if prefix_end >= bytes.len() || bytes[prefix_end] != b'-' {
			continue;
		}

		let mut digit_end = prefix_end + 1;

		while digit_end < bytes.len() && bytes[digit_end].is_ascii_digit() {
			digit_end += 1;
		}

		if digit_end > prefix_end + 1 {
			return Some(value[index..digit_end].to_ascii_uppercase());
		}
	}

	None
}

fn ghost_lane_identifier_suffix_matches(left: &str, right: &str) -> bool {
	let Some((left_prefix, left_suffix)) = ghost_lane_identifier_parts(left) else {
		return false;
	};
	let Some((right_prefix, right_suffix)) = ghost_lane_identifier_parts(right) else {
		return false;
	};

	left_suffix == right_suffix
		&& (left_prefix.eq_ignore_ascii_case(right_prefix)
			|| left_prefix.to_ascii_uppercase().starts_with(&right_prefix.to_ascii_uppercase())
			|| right_prefix.to_ascii_uppercase().starts_with(&left_prefix.to_ascii_uppercase()))
}

fn ghost_lane_identifier_parts(value: &str) -> Option<(&str, &str)> {
	let (prefix, suffix) = value.rsplit_once('-')?;

	(!prefix.is_empty() && suffix.chars().all(|character| character.is_ascii_digit()))
		.then_some((prefix, suffix))
}

fn sorted_unique(values: Vec<String>) -> Vec<String> {
	let mut set = BTreeSet::new();

	for value in values {
		set.insert(value);
	}

	set.into_iter().collect()
}
