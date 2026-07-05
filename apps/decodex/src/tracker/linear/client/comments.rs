use crate::{
	prelude::{Result, eyre},
	tracker::{
		TrackerComment,
		linear::{
			LinearClient, mapping,
			queries::ISSUE_COMMENTS_QUERY,
			schema::{IssueCommentsData, IssueCommentsVariables},
		},
	},
};

impl LinearClient {
	pub(in crate::tracker::linear) fn collect_issue_comments(
		&self,
		issue_id: &str,
	) -> Result<Vec<TrackerComment>> {
		let mut after = None;
		let mut comments = Vec::new();

		loop {
			let data = self.post::<_, IssueCommentsData>(
				ISSUE_COMMENTS_QUERY,
				&IssueCommentsVariables { issue_id: issue_id.to_owned(), after: after.clone() },
			)?;
			let Some(issue) = data.issue else {
				eyre::bail!("Linear did not return issue `{issue_id}` while listing comments.");
			};
			let connection = issue.comments;

			comments.extend(connection.nodes.into_iter().map(|comment| TrackerComment {
				body: comment.body,
				created_at: comment.created_at,
			}));

			if !connection.page_info.has_next_page {
				break;
			}

			after = Some(mapping::require_end_cursor(
				connection.page_info,
				"Linear comment pagination reported `hasNextPage = true` without an `endCursor`.",
			)?);
		}

		Ok(comments)
	}
}
