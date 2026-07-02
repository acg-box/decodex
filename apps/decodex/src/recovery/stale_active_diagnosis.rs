//! Diagnostic assembly for stale-active recovery.

mod inspection;

use std::path::Path;

use crate::{
	commit_message,
	prelude::{Result, eyre},
	recovery::{context::RecoveryRuntimeMutationPolicy, reports::StaleActiveDiagnostic},
	state::StateStore,
	tracker::{self, IssueTracker, TrackerIssue},
	workflow::WorkflowDocument,
};

pub(super) fn diagnose_stale_active_issues<T>(
	project_id: &str,
	workflow: &WorkflowDocument,
	worktree_root: &Path,
	state_store: &StateStore,
	tracker: &T,
	selector: Option<&str>,
	listing_mode: RecoveryRuntimeMutationPolicy,
) -> Result<Vec<StaleActiveDiagnostic>>
where
	T: IssueTracker + ?Sized,
{
	let issues = if let Some(selector) = selector {
		vec![lookup_stale_active_issue(tracker, selector)?]
	} else {
		tracker.list_issues_with_label(&tracker::automation_active_label(project_id))?
	};

	issues
		.into_iter()
		.map(|issue| {
			inspection::inspect_stale_active_issue(
				project_id,
				workflow,
				worktree_root,
				state_store,
				tracker,
				issue,
				listing_mode,
			)
		})
		.collect()
}

pub(super) fn lookup_stale_active_issue<T>(tracker: &T, selector: &str) -> Result<TrackerIssue>
where
	T: IssueTracker + ?Sized,
{
	let selector = selector.trim();

	if selector.is_empty() {
		eyre::bail!("Issue selector must not be empty.");
	}
	if commit_message::looks_like_issue_identifier(selector) {
		return tracker
			.get_issue_by_identifier(selector)?
			.ok_or_else(|| eyre::eyre!("No tracker issue matched `{selector}`."));
	}

	if let Some(issue) = tracker.refresh_issues(&[selector.to_owned()])?.pop() {
		return Ok(issue);
	}

	tracker
		.get_issue_by_identifier(selector)?
		.ok_or_else(|| eyre::eyre!("No tracker issue matched `{selector}`."))
}
