pub(crate) mod issue_tracker;
pub(crate) mod mapping;
pub(crate) mod queries;
pub(crate) mod schema;
pub(crate) mod transport;

use std::time::Duration;

use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};

use crate::{
	prelude::{Result, eyre},
	tracker::{
		TrackerComment, TrackerIssue, TrackerIssueBlocker,
		linear::{
			queries::{
				ISSUE_ARCHIVE_MUTATION, ISSUE_BLOCKERS_QUERY, ISSUE_COMMENTS_QUERY,
				LINEAR_GRAPHQL_URL,
			},
			schema::{
				GraphqlRequest, GraphqlResponse, IssueArchiveData, IssueArchiveVariables,
				IssueBlockersData, IssueBlockersVariables, IssueCommentsData,
				IssueCommentsVariables, IssueConnectionData, LinearIssue,
			},
		},
	},
};

const LINEAR_HTTP_TIMEOUT: Duration = Duration::from_secs(30);
const LINEAR_HTTP_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) struct LinearClient {
	api_token: String,
	http: Client,
}
impl LinearClient {
	pub(crate) fn new(api_token: String) -> Result<Self> {
		let http = Client::builder()
			.connect_timeout(LINEAR_HTTP_CONNECT_TIMEOUT)
			.timeout(LINEAR_HTTP_TIMEOUT)
			.build()?;

		Ok(Self { api_token, http })
	}

	pub(crate) fn archive_issue(&self, issue_id: &str) -> Result<()> {
		let data = self.post::<_, IssueArchiveData>(
			ISSUE_ARCHIVE_MUTATION,
			&IssueArchiveVariables { id: issue_id, trash: false },
		)?;

		if !data.issue_archive.success {
			eyre::bail!("Linear did not confirm the issue archive mutation.");
		}

		Ok(())
	}

	fn post<V, T>(&self, query: &str, variables: &V) -> Result<T>
	where
		V: Serialize,
		T: for<'de> Deserialize<'de>,
	{
		let response = self
			.http
			.post(LINEAR_GRAPHQL_URL)
			.header("Authorization", &self.api_token)
			.json(&GraphqlRequest { query, variables })
			.send()
			.map_err(transport::linear_transport_error)?;
		let status = response.status();
		let body = response.text().map_err(transport::linear_transport_error)?;
		let payload = serde_json::from_str::<GraphqlResponse<T>>(&body).map_err(|error| {
			if status.is_success() {
				eyre::eyre!("Failed to parse Linear GraphQL response: {error}")
			} else {
				eyre::eyre!(
					"Linear HTTP request failed with status `{}` and an unparseable GraphQL body: {error}",
					status
				)
			}
		})?;

		if let Some(errors) = payload.errors {
			if let Some(message) = transport::rate_limited_error_message(&errors) {
				eyre::bail!("{message}");
			}

			let messages =
				errors.into_iter().map(|error| error.message).collect::<Vec<_>>().join("; ");

			eyre::bail!("Linear GraphQL request failed: {messages}");
		}

		if !status.is_success() {
			eyre::bail!("Linear HTTP request failed with status `{status}`.");
		}

		payload.data.ok_or_else(|| eyre::eyre!("Linear GraphQL response did not include data."))
	}

	fn collect_issue_pages<V, F>(
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

	fn resolve_issue_blockers(&self, issue: &LinearIssue) -> Result<Vec<TrackerIssueBlocker>> {
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

	fn collect_issue_comments(&self, issue_id: &str) -> Result<Vec<TrackerComment>> {
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

#[cfg(test)]
mod tests;
