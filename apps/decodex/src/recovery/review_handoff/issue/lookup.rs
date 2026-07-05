use crate::{
	prelude::{Result, eyre},
	tracker::{IssueTracker, TrackerIssue},
};

pub(in crate::recovery) fn load_issue_by_identifier<T>(
	tracker: &T,
	issue_identifier: &str,
) -> Result<TrackerIssue>
where
	T: IssueTracker + ?Sized,
{
	tracker
		.get_issue_by_identifier(issue_identifier)?
		.ok_or_else(|| eyre::eyre!("Tracker issue `{issue_identifier}` was not found."))
}
