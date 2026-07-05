use crate::{
	prelude::Result,
	tracker::{
		TrackerIssue,
		linear::{
			LinearClient, mapping,
			queries::{
				ISSUE_BY_IDENTIFIER_QUERY, ISSUES_BY_IDS_QUERY, ISSUES_WITH_LABEL_QUERY,
				TEAM_LABEL_BY_NAME_QUERY,
			},
			schema::{
				IssueByIdentifierData, IssueByIdentifierVariables, IssuesByIdsVariables,
				IssuesWithLabelVariables, TeamLabelByNameData, TeamLabelByNameVariables,
			},
		},
	},
};

pub(in crate::tracker::linear::issue_tracker) fn list_issues_with_label(
	client: &LinearClient,
	label_name: &str,
) -> Result<Vec<TrackerIssue>> {
	client.collect_issue_pages(ISSUES_WITH_LABEL_QUERY, |after| IssuesWithLabelVariables {
		label_name: label_name.to_owned(),
		after,
	})
}

pub(in crate::tracker::linear::issue_tracker) fn find_team_label_id(
	client: &LinearClient,
	team_id: &str,
	label_name: &str,
) -> Result<Option<String>> {
	let data = client.post::<_, TeamLabelByNameData>(
		TEAM_LABEL_BY_NAME_QUERY,
		&TeamLabelByNameVariables {
			team_id: team_id.to_owned(),
			label_name: label_name.to_owned(),
		},
	)?;

	Ok(data.issue_labels.nodes.into_iter().next().map(|label| label.id))
}

pub(in crate::tracker::linear::issue_tracker) fn get_issue_by_identifier(
	client: &LinearClient,
	issue_identifier: &str,
) -> Result<Option<TrackerIssue>> {
	let data = client.post::<_, IssueByIdentifierData>(
		ISSUE_BY_IDENTIFIER_QUERY,
		&IssueByIdentifierVariables { issue_identifier: issue_identifier.to_owned() },
	)?;
	let Some(issue) = data.issue else {
		return Ok(None);
	};
	let blockers = client.resolve_issue_blockers(&issue)?;

	Ok(Some(mapping::map_issue(issue, blockers)))
}

pub(in crate::tracker::linear::issue_tracker) fn refresh_issues(
	client: &LinearClient,
	issue_ids: &[String],
) -> Result<Vec<TrackerIssue>> {
	if issue_ids.is_empty() {
		return Ok(Vec::new());
	}

	client.collect_issue_pages(ISSUES_BY_IDS_QUERY, |after| IssuesByIdsVariables {
		issue_ids: issue_ids.to_vec(),
		after,
	})
}
