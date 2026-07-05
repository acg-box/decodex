use serde::Serialize;

use crate::{
	prelude::Result,
	tracker::{
		TrackerIssue,
		linear::{LinearClient, mapping, schema::IssueConnectionData},
	},
};

impl LinearClient {
	pub(in crate::tracker::linear) fn collect_issue_pages<V, F>(
		&self,
		query: &str,
		mut make_variables: F,
	) -> Result<Vec<TrackerIssue>>
	where
		V: Serialize,
		F: FnMut(Option<String>) -> V,
	{
		let mut after = None;
		let mut issues = Vec::new();

		loop {
			let data =
				self.post::<_, IssueConnectionData>(query, &make_variables(after.clone()))?;
			let connection = data.issues;

			for issue in connection.nodes {
				let blockers = self.resolve_issue_blockers(&issue)?;

				issues.push(mapping::map_issue(issue, blockers));
			}

			if !connection.page_info.has_next_page {
				break;
			}

			after = Some(mapping::require_end_cursor(
				connection.page_info,
				"Linear issue pagination reported `hasNextPage = true` without an `endCursor`.",
			)?);
		}

		Ok(issues)
	}
}
