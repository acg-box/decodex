use crate::{
	commit_message,
	orchestrator::run_cycle::{IssueTracker, Result},
};

pub(crate) fn resolve_target_issue_id<T>(tracker: &T, issue_reference: &str) -> Result<String>
where
	T: IssueTracker,
{
	if commit_message::looks_like_issue_identifier(issue_reference)
		&& let Some(issue) = tracker.get_issue_by_identifier(issue_reference)?
	{
		return Ok(issue.id);
	}

	Ok(issue_reference.to_owned())
}
