use crate::{
	prelude::{Result, eyre},
	tracker::{
		TrackerIssueBlocker,
		linear::{
			LinearClient, mapping,
			queries::ISSUE_BLOCKERS_QUERY,
			schema::{IssueBlockersData, IssueBlockersVariables, LinearIssue},
		},
	},
};

impl LinearClient {
	pub(in crate::tracker::linear) fn resolve_issue_blockers(
		&self,
		issue: &LinearIssue,
	) -> Result<Vec<TrackerIssueBlocker>> {
		let mut blockers = mapping::map_blockers(&issue.inverse_relations.nodes);

		if issue.state.name != "Todo" || !issue.inverse_relations.page_info.has_next_page {
			return Ok(blockers);
		}

		let mut after = Some(mapping::require_end_cursor(
			issue.inverse_relations.page_info.clone(),
			"Linear blocker pagination reported `hasNextPage = true` without an `endCursor`.",
		)?);

		while let Some(cursor) = after {
			let data = self.post::<_, IssueBlockersData>(
				ISSUE_BLOCKERS_QUERY,
				&IssueBlockersVariables { issue_id: issue.id.clone(), after: Some(cursor) },
			)?;
			let Some(issue_page) = data.issues.nodes.into_iter().next() else {
				eyre::bail!(
					"Linear blocker pagination did not return the requested issue `{}`.",
					issue.id
				);
			};
			let blocker_page = issue_page.inverse_relations;

			blockers.extend(mapping::map_blockers(&blocker_page.nodes));

			after = if blocker_page.page_info.has_next_page {
				Some(mapping::require_end_cursor(
					blocker_page.page_info,
					"Linear blocker pagination reported `hasNextPage = true` without an `endCursor`.",
				)?)
			} else {
				None
			};
		}

		Ok(blockers)
	}
}
