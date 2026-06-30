use std::time::Duration;

use color_eyre::Report;
use reqwest::{Error, blocking::Client};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
	prelude::{Result, eyre},
	tracker::{
		IssueTracker, TrackerComment, TrackerIssue, TrackerIssueBlocker, TrackerIssueBriefUpdate,
		TrackerIssueCreate, TrackerLabel, TrackerState, TrackerTeam,
	},
};

mod queries;

use queries::{
	COMMENT_CREATE_MUTATION, ISSUE_ARCHIVE_MUTATION, ISSUE_BLOCKERS_QUERY,
	ISSUE_BY_IDENTIFIER_QUERY, ISSUE_COMMENTS_QUERY, ISSUE_CREATE_MUTATION,
	ISSUE_UPDATE_BRIEF_MUTATION, ISSUE_UPDATE_MUTATION, ISSUES_BY_IDS_QUERY,
	ISSUES_WITH_LABEL_QUERY, LINEAR_GRAPHQL_URL, TEAM_LABEL_BY_NAME_QUERY,
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
			.map_err(linear_transport_error)?;
		let status = response.status();
		let body = response.text().map_err(linear_transport_error)?;
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
			if let Some(message) = rate_limited_error_message(&errors) {
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

				issues.push(map_issue(issue, blockers));
			}

			if !connection.page_info.has_next_page {
				break;
			}

			after = Some(require_end_cursor(
				connection.page_info,
				"Linear issue pagination reported `hasNextPage = true` without an `endCursor`.",
			)?);
		}

		Ok(issues)
	}

	fn resolve_issue_blockers(&self, issue: &LinearIssue) -> Result<Vec<TrackerIssueBlocker>> {
		let mut blockers = map_blockers(&issue.inverse_relations.nodes);

		if issue.state.name != "Todo" || !issue.inverse_relations.page_info.has_next_page {
			return Ok(blockers);
		}

		let mut after = Some(require_end_cursor(
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

			blockers.extend(map_blockers(&blocker_page.nodes));

			after = if blocker_page.page_info.has_next_page {
				Some(require_end_cursor(
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

			after = Some(require_end_cursor(
				connection.page_info,
				"Linear comment pagination reported `hasNextPage = true` without an `endCursor`.",
			)?);
		}

		Ok(comments)
	}
}

impl IssueTracker for LinearClient {
	fn list_issues_with_label(&self, label_name: &str) -> Result<Vec<TrackerIssue>> {
		self.collect_issue_pages(ISSUES_WITH_LABEL_QUERY, |after| IssuesWithLabelVariables {
			label_name: label_name.to_owned(),
			after,
		})
	}

	fn find_team_label_id(&self, team_id: &str, label_name: &str) -> Result<Option<String>> {
		let data = self.post::<_, TeamLabelByNameData>(
			TEAM_LABEL_BY_NAME_QUERY,
			&TeamLabelByNameVariables {
				team_id: team_id.to_owned(),
				label_name: label_name.to_owned(),
			},
		)?;

		Ok(data.issue_labels.nodes.into_iter().next().map(|label| label.id))
	}

	fn get_issue_by_identifier(&self, issue_identifier: &str) -> Result<Option<TrackerIssue>> {
		let data = self.post::<_, IssueByIdentifierData>(
			ISSUE_BY_IDENTIFIER_QUERY,
			&IssueByIdentifierVariables { issue_identifier: issue_identifier.to_owned() },
		)?;
		let Some(issue) = data.issue else {
			return Ok(None);
		};
		let blockers = self.resolve_issue_blockers(&issue)?;

		Ok(Some(map_issue(issue, blockers)))
	}

	fn refresh_issues(&self, issue_ids: &[String]) -> Result<Vec<TrackerIssue>> {
		if issue_ids.is_empty() {
			return Ok(Vec::new());
		}

		self.collect_issue_pages(ISSUES_BY_IDS_QUERY, |after| IssuesByIdsVariables {
			issue_ids: issue_ids.to_vec(),
			after,
		})
	}

	fn list_comments(&self, issue_id: &str) -> Result<Vec<TrackerComment>> {
		self.collect_issue_comments(issue_id)
	}

	fn update_issue_state(&self, issue_id: &str, state_id: &str) -> Result<()> {
		let data = self.post::<_, IssueUpdateData>(
			ISSUE_UPDATE_MUTATION,
			&IssueUpdateVariables {
				id: issue_id,
				input: IssueUpdateInput {
					title: None,
					description: None,
					state_id: Some(state_id.to_owned()),
					label_ids: None,
					added_label_ids: None,
					removed_label_ids: None,
				},
			},
		)?;

		if !data.issue_update.success {
			eyre::bail!("Linear did not confirm the issue state update.");
		}

		Ok(())
	}

	fn add_issue_labels(&self, issue_id: &str, label_ids: &[String]) -> Result<()> {
		let data = self.post::<_, IssueUpdateData>(
			ISSUE_UPDATE_MUTATION,
			&IssueUpdateVariables {
				id: issue_id,
				input: IssueUpdateInput {
					title: None,
					description: None,
					state_id: None,
					label_ids: None,
					added_label_ids: Some(label_ids.to_vec()),
					removed_label_ids: None,
				},
			},
		)?;

		if !data.issue_update.success {
			eyre::bail!("Linear did not confirm the issue label addition.");
		}

		Ok(())
	}

	fn remove_issue_labels(&self, issue_id: &str, label_ids: &[String]) -> Result<()> {
		let data = self.post::<_, IssueUpdateData>(
			ISSUE_UPDATE_MUTATION,
			&IssueUpdateVariables {
				id: issue_id,
				input: IssueUpdateInput {
					title: None,
					description: None,
					state_id: None,
					label_ids: None,
					added_label_ids: None,
					removed_label_ids: Some(label_ids.to_vec()),
				},
			},
		)?;

		if !data.issue_update.success {
			eyre::bail!("Linear did not confirm the issue label removal.");
		}

		Ok(())
	}

	fn create_issue(&self, request: &TrackerIssueCreate) -> Result<TrackerIssue> {
		let data = self.post::<_, IssueCreateData>(
			ISSUE_CREATE_MUTATION,
			&IssueCreateVariables {
				input: IssueCreateInput {
					team_id: request.team_id.clone(),
					title: request.title.clone(),
					description: request.description.clone(),
					state_id: request.state_id.clone(),
				},
			},
		)?;

		if !data.issue_create.success {
			eyre::bail!("Linear did not confirm the issue creation.");
		}

		let issue = data
			.issue_create
			.issue
			.ok_or_else(|| eyre::eyre!("Linear issue creation response did not include issue."))?;
		let blockers = self.resolve_issue_blockers(&issue)?;

		Ok(map_issue(issue, blockers))
	}

	fn update_issue_brief(
		&self,
		issue_id: &str,
		request: &TrackerIssueBriefUpdate,
	) -> Result<TrackerIssue> {
		let data = self.post::<_, IssueUpdateWithIssueData>(
			ISSUE_UPDATE_BRIEF_MUTATION,
			&IssueUpdateVariables {
				id: issue_id,
				input: IssueUpdateInput {
					title: Some(request.title.clone()),
					description: Some(request.description.clone()),
					state_id: None,
					label_ids: None,
					added_label_ids: None,
					removed_label_ids: None,
				},
			},
		)?;

		if !data.issue_update.success {
			eyre::bail!("Linear did not confirm the issue brief update.");
		}

		let issue = data
			.issue_update
			.issue
			.ok_or_else(|| eyre::eyre!("Linear issue update response did not include issue."))?;
		let blockers = self.resolve_issue_blockers(&issue)?;

		Ok(map_issue(issue, blockers))
	}

	fn create_comment(&self, issue_id: &str, body: &str) -> Result<()> {
		let data = self.post::<_, CommentCreateData>(
			COMMENT_CREATE_MUTATION,
			&CommentCreateVariables {
				input: CommentCreateInput { body: body.to_owned(), issue_id: issue_id.to_owned() },
			},
		)?;

		if !data.comment_create.success {
			eyre::bail!("Linear did not confirm the comment creation.");
		}

		Ok(())
	}
}

#[derive(Serialize)]
struct GraphqlRequest<'a, V> {
	query: &'a str,
	variables: V,
}

#[derive(Deserialize)]
struct GraphqlResponse<T> {
	data: Option<T>,
	errors: Option<Vec<GraphqlError>>,
}

#[derive(Deserialize)]
struct GraphqlError {
	message: String,
	extensions: Option<Value>,
}

#[derive(Serialize)]
struct IssuesWithLabelVariables {
	#[serde(rename = "labelName")]
	label_name: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	after: Option<String>,
}

#[derive(Serialize)]
struct IssueByIdentifierVariables {
	#[serde(rename = "issueIdentifier")]
	issue_identifier: String,
}

#[derive(Serialize)]
struct IssuesByIdsVariables {
	#[serde(rename = "issueIds")]
	issue_ids: Vec<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	after: Option<String>,
}

#[derive(Serialize)]
struct IssueBlockersVariables {
	#[serde(rename = "issueId")]
	issue_id: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	after: Option<String>,
}

#[derive(Serialize)]
struct IssueCommentsVariables {
	#[serde(rename = "issueId")]
	issue_id: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	after: Option<String>,
}

#[derive(Deserialize)]
struct IssueConnectionData {
	issues: IssueConnection,
}

#[derive(Deserialize)]
struct IssueByIdentifierData {
	issue: Option<LinearIssue>,
}

#[derive(Deserialize)]
struct IssueBlockersData {
	issues: IssueBlockerConnection,
}

#[derive(Deserialize)]
struct IssueCommentsData {
	issue: Option<LinearIssueComments>,
}

#[derive(Deserialize)]
struct IssueConnection {
	nodes: Vec<LinearIssue>,
	#[serde(rename = "pageInfo")]
	page_info: PageInfo,
}

#[derive(Deserialize)]
struct IssueBlockerConnection {
	nodes: Vec<LinearIssueBlockerPage>,
}

#[derive(Deserialize)]
struct LinearIssueBlockerPage {
	#[serde(rename = "inverseRelations")]
	inverse_relations: IssueRelationConnection,
}

#[derive(Deserialize)]
struct LinearIssueComments {
	comments: CommentConnection,
}

#[derive(Deserialize)]
struct CommentConnection {
	nodes: Vec<LinearComment>,
	#[serde(rename = "pageInfo")]
	page_info: PageInfo,
}

#[derive(Deserialize)]
struct LinearComment {
	body: String,
	#[serde(rename = "createdAt")]
	created_at: String,
}

#[derive(Clone, Deserialize)]
struct PageInfo {
	#[serde(rename = "hasNextPage")]
	has_next_page: bool,
	#[serde(rename = "endCursor")]
	end_cursor: Option<String>,
}

#[derive(Deserialize)]
struct LinearIssue {
	id: String,
	identifier: String,
	title: String,
	creator: Option<LinearUser>,
	description: Option<String>,
	priority: Option<i64>,
	#[serde(rename = "createdAt")]
	created_at: String,
	#[serde(rename = "updatedAt")]
	updated_at: String,
	state: LinearState,
	team: LinearTeam,
	labels: LabelConnection,
	#[serde(rename = "inverseRelations")]
	inverse_relations: IssueRelationConnection,
}

#[derive(Deserialize)]
struct LinearTeam {
	id: String,
	name: String,
	states: StateConnection,
	labels: LabelConnection,
}

#[derive(Deserialize)]
struct LinearUser {
	#[serde(rename = "displayName")]
	display_name: Option<String>,
	name: Option<String>,
	email: Option<String>,
}

#[derive(Deserialize)]
struct StateConnection {
	nodes: Vec<LinearState>,
}

#[derive(Deserialize)]
struct LabelConnection {
	nodes: Vec<LinearLabel>,
	#[serde(rename = "pageInfo")]
	page_info: Option<PageInfo>,
}

#[derive(Deserialize)]
struct IssueRelationConnection {
	nodes: Vec<LinearIssueRelation>,
	#[serde(rename = "pageInfo")]
	page_info: PageInfo,
}

#[derive(Deserialize)]
struct LinearIssueRelation {
	#[serde(rename = "type")]
	relation_type: String,
	issue: LinearRelatedIssue,
}

#[derive(Deserialize)]
struct LinearRelatedIssue {
	id: String,
	identifier: String,
	state: LinearState,
}

#[derive(Deserialize)]
struct LinearState {
	id: String,
	name: String,
}

#[derive(Deserialize)]
struct LinearLabel {
	id: String,
	name: String,
}

#[derive(Serialize)]
struct IssueUpdateVariables<'a> {
	id: &'a str,
	input: IssueUpdateInput,
}

#[derive(Serialize)]
struct IssueUpdateInput {
	#[serde(skip_serializing_if = "Option::is_none")]
	title: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	description: Option<String>,
	#[serde(rename = "stateId", skip_serializing_if = "Option::is_none")]
	state_id: Option<String>,
	#[serde(rename = "labelIds", skip_serializing_if = "Option::is_none")]
	label_ids: Option<Vec<String>>,
	#[serde(rename = "addedLabelIds", skip_serializing_if = "Option::is_none")]
	added_label_ids: Option<Vec<String>>,
	#[serde(rename = "removedLabelIds", skip_serializing_if = "Option::is_none")]
	removed_label_ids: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct IssueUpdateData {
	#[serde(rename = "issueUpdate")]
	issue_update: MutationSuccess,
}

#[derive(Deserialize)]
struct IssueUpdateWithIssueData {
	#[serde(rename = "issueUpdate")]
	issue_update: IssueMutationWithIssue,
}

#[derive(Serialize)]
struct IssueCreateVariables {
	input: IssueCreateInput,
}

#[derive(Serialize)]
struct IssueCreateInput {
	#[serde(rename = "teamId")]
	team_id: String,
	title: String,
	description: String,
	#[serde(rename = "stateId", skip_serializing_if = "Option::is_none")]
	state_id: Option<String>,
}

#[derive(Deserialize)]
struct IssueCreateData {
	#[serde(rename = "issueCreate")]
	issue_create: IssueMutationWithIssue,
}

#[derive(Deserialize)]
struct IssueMutationWithIssue {
	success: bool,
	issue: Option<LinearIssue>,
}

#[derive(Serialize)]
struct IssueArchiveVariables<'a> {
	id: &'a str,
	trash: bool,
}

#[derive(Deserialize)]
struct IssueArchiveData {
	#[serde(rename = "issueArchive")]
	issue_archive: MutationSuccess,
}

#[derive(Deserialize)]
struct MutationSuccess {
	success: bool,
}

#[derive(Serialize)]
struct TeamLabelByNameVariables {
	#[serde(rename = "teamId")]
	team_id: String,
	#[serde(rename = "labelName")]
	label_name: String,
}

#[derive(Deserialize)]
struct TeamLabelByNameData {
	#[serde(rename = "issueLabels")]
	issue_labels: LabelConnection,
}

#[derive(Serialize)]
struct CommentCreateVariables {
	input: CommentCreateInput,
}

#[derive(Serialize)]
struct CommentCreateInput {
	body: String,
	#[serde(rename = "issueId")]
	issue_id: String,
}

#[derive(Deserialize)]
struct CommentCreateData {
	#[serde(rename = "commentCreate")]
	comment_create: MutationSuccess,
}

fn linear_transport_error(error: Error) -> Report {
	if error.is_timeout() {
		eyre::eyre!("Linear connector timed out during GraphQL request: {error}")
	} else {
		Report::new(error)
	}
}

fn rate_limited_error_message(errors: &[GraphqlError]) -> Option<String> {
	errors.iter().find_map(|error| {
		let extensions = error.extensions.as_ref()?;
		let code = extensions.get("code").and_then(Value::as_str)?;

		if code != "RATELIMITED" {
			return None;
		}

		let user_message = extensions
			.get("userPresentableMessage")
			.and_then(Value::as_str)
			.unwrap_or(error.message.as_str());
		let reset = extensions.get("reset").and_then(Value::as_i64);

		Some(match reset {
			Some(reset) => {
				format!("Linear connector is rate limited until `{reset}`: {user_message}")
			},
			None => format!("Linear connector is rate limited: {user_message}"),
		})
	})
}

fn require_end_cursor(page_info: PageInfo, message: &str) -> Result<String> {
	page_info.end_cursor.ok_or_else(|| eyre::eyre!(message.to_owned()))
}

fn map_blockers(relations: &[LinearIssueRelation]) -> Vec<TrackerIssueBlocker> {
	relations
		.iter()
		.filter(|relation| relation.relation_type == "blocks")
		.map(|relation| TrackerIssueBlocker {
			id: relation.issue.id.clone(),
			identifier: relation.issue.identifier.clone(),
			state: TrackerState {
				id: relation.issue.state.id.clone(),
				name: relation.issue.state.name.clone(),
			},
		})
		.collect()
}

fn map_issue(issue: LinearIssue, blockers: Vec<TrackerIssueBlocker>) -> TrackerIssue {
	let author = linear_user_display_name(issue.creator.as_ref());

	TrackerIssue {
		id: issue.id,
		identifier: issue.identifier,
		#[cfg(test)]
		project_slug: None,
		title: issue.title,
		author,
		description: issue.description.unwrap_or_default(),
		priority: issue.priority,
		created_at: issue.created_at,
		updated_at: issue.updated_at,
		state: TrackerState { id: issue.state.id, name: issue.state.name },
		team: TrackerTeam {
			id: issue.team.id,
			name: issue.team.name,
			states: issue
				.team
				.states
				.nodes
				.into_iter()
				.map(|state| TrackerState { id: state.id, name: state.name })
				.collect(),
			labels: issue
				.team
				.labels
				.nodes
				.into_iter()
				.map(|label| TrackerLabel { id: label.id, name: label.name })
				.collect(),
		},
		labels_complete: issue.labels.page_info.is_none_or(|page_info| !page_info.has_next_page),
		labels: issue
			.labels
			.nodes
			.into_iter()
			.map(|label| TrackerLabel { id: label.id, name: label.name })
			.collect(),
		blockers,
	}
}

fn linear_user_display_name(user: Option<&LinearUser>) -> Option<String> {
	let user = user?;

	[&user.display_name, &user.name, &user.email]
		.into_iter()
		.filter_map(|value| value.as_deref())
		.map(str::trim)
		.find(|value| !value.is_empty())
		.map(str::to_owned)
}

#[cfg(test)] mod tests;
