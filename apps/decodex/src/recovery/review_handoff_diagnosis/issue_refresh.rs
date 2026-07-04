use std::collections::HashMap;

use crate::{
	prelude::Result,
	state::WorktreeMapping,
	tracker::{IssueTracker, TrackerIssue},
};

pub(in crate::recovery::review_handoff_diagnosis) fn refresh_retained_review_worktree_issues<T>(
	tracker: &T,
	worktrees: &[WorktreeMapping],
) -> Result<HashMap<String, TrackerIssue>>
where
	T: IssueTracker,
{
	if worktrees.is_empty() {
		return Ok(HashMap::new());
	}

	let issue_ids =
		worktrees.iter().map(|worktree| worktree.issue_id().to_owned()).collect::<Vec<_>>();

	Ok(tracker
		.refresh_issues(&issue_ids)?
		.into_iter()
		.map(|issue| (issue.id.clone(), issue))
		.collect())
}
