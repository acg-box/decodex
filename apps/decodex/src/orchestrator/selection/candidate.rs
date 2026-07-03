use std::cmp::Ordering;

use crate::{
	orchestrator::{self, IssueTracker, StateStore, TrackerIssue},
	prelude::Result,
	workflow::WorkflowDocument,
};

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn select_issue_candidate(
	tracker: &dyn IssueTracker,
	issues: Vec<TrackerIssue>,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	project_id: &str,
) -> Result<Option<TrackerIssue>> {
	select_issue_candidate_with_exclusions(tracker, issues, workflow, state_store, project_id, &[])
}

pub(crate) fn select_issue_candidate_with_exclusions(
	tracker: &dyn IssueTracker,
	issues: Vec<TrackerIssue>,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	project_id: &str,
	excluded_issue_ids: &[&str],
) -> Result<Option<TrackerIssue>> {
	let mut eligible_issues = Vec::new();

	for issue in issues {
		if excluded_issue_ids.contains(&issue.id.as_str()) {
			continue;
		}
		if state_store.issue_has_active_shared_claim(project_id, &issue.id)? {
			continue;
		}
		if orchestrator::is_issue_eligible(tracker, &issue, project_id, workflow, state_store)? {
			eligible_issues.push(issue);
		}
	}

	eligible_issues.sort_by(compare_issue_candidates);

	Ok(eligible_issues.into_iter().next())
}

pub(crate) fn compare_issue_candidates(left: &TrackerIssue, right: &TrackerIssue) -> Ordering {
	let left_priority = (left.priority.is_none(), left.priority.unwrap_or(i64::MAX));
	let right_priority = (right.priority.is_none(), right.priority.unwrap_or(i64::MAX));

	left_priority
		.cmp(&right_priority)
		.then_with(|| left.created_at.cmp(&right.created_at))
		.then_with(|| left.identifier.cmp(&right.identifier))
}
