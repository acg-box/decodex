use crate::{
	prelude::{Result, eyre},
	tracker::linear::{
		LinearClient,
		queries::{ISSUE_INVERSE_RELATIONS_QUERY, ISSUE_RELATIONS_QUERY},
		schema::{
			ExplicitIssueRelationConnection, IssueInverseRelationsData, IssueRelationsData,
			IssueRelationsVariables,
		},
	},
};

impl LinearClient {
	pub(in crate::tracker::linear) fn inspect_explicit_issue_relation(
		&self,
		issue_id: &str,
		related_issue_id: &str,
	) -> Result<bool> {
		if self.direct_issue_relations_contain(issue_id, related_issue_id)? {
			return Ok(true);
		}

		self.inverse_issue_relations_contain(issue_id, related_issue_id)
	}

	fn direct_issue_relations_contain(
		&self,
		issue_id: &str,
		related_issue_id: &str,
	) -> Result<bool> {
		let mut after = None;

		loop {
			let data = self.post::<_, IssueRelationsData>(
				ISSUE_RELATIONS_QUERY,
				&IssueRelationsVariables { issue_id: issue_id.to_owned(), after },
			)?;
			let issue = data.issue.ok_or_else(|| {
				eyre::eyre!(
					"Linear did not return issue `{issue_id}` while checking issue lineage."
				)
			})?;

			if relation_connection_contains(&issue.relations, issue_id, related_issue_id) {
				return Ok(true);
			}

			after = next_relation_cursor(issue.relations, issue_id)?;
			if after.is_none() {
				return Ok(false);
			}
		}
	}

	fn inverse_issue_relations_contain(
		&self,
		issue_id: &str,
		related_issue_id: &str,
	) -> Result<bool> {
		let mut after = None;

		loop {
			let data = self.post::<_, IssueInverseRelationsData>(
				ISSUE_INVERSE_RELATIONS_QUERY,
				&IssueRelationsVariables { issue_id: issue_id.to_owned(), after },
			)?;
			let issue = data.issue.ok_or_else(|| {
				eyre::eyre!(
					"Linear did not return issue `{issue_id}` while checking issue lineage."
				)
			})?;

			if relation_connection_contains(&issue.inverse_relations, issue_id, related_issue_id) {
				return Ok(true);
			}

			after = next_relation_cursor(issue.inverse_relations, issue_id)?;
			if after.is_none() {
				return Ok(false);
			}
		}
	}
}

fn relation_connection_contains(
	connection: &ExplicitIssueRelationConnection,
	issue_id: &str,
	related_issue_id: &str,
) -> bool {
	connection.nodes.iter().any(|relation| {
		(relation.issue.id == issue_id && relation.related_issue.id == related_issue_id)
			|| (relation.issue.id == related_issue_id && relation.related_issue.id == issue_id)
	})
}

fn next_relation_cursor(
	connection: ExplicitIssueRelationConnection,
	issue_id: &str,
) -> Result<Option<String>> {
	if !connection.page_info.has_next_page {
		return Ok(None);
	}

	connection.page_info.end_cursor.map(Some).ok_or_else(|| {
		eyre::eyre!(
			"Linear issue `{issue_id}` relation pagination reported `hasNextPage = true` without an `endCursor`."
		)
	})
}
