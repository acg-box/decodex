use crate::orchestrator::status::post_review::{
	HashMap, OperatorStatusSnapshot, TrackerIssue, WorktreeMapping,
};

pub(crate) fn hydrate_worktree_issue_metadata(
	snapshot: &mut OperatorStatusSnapshot,
	worktree_issues: &[(WorktreeMapping, TrackerIssue)],
) {
	let issues_by_id = worktree_issues
		.iter()
		.map(|(_, issue)| (issue.id.as_str(), issue))
		.collect::<HashMap<_, _>>();

	for worktree in &mut snapshot.worktrees {
		let Some(issue) = issues_by_id.get(worktree.issue_id.as_str()) else {
			continue;
		};

		worktree.issue_identifier = Some(issue.identifier.clone());
		worktree.issue_state = Some(issue.state.name.clone());
	}
}
