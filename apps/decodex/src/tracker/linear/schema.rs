use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Serialize)]
pub(super) struct GraphqlRequest<'a, V> {
	pub(super) query: &'a str,
	pub(super) variables: V,
}

#[derive(Deserialize)]
pub(super) struct GraphqlResponse<T> {
	pub(super) data: Option<T>,
	pub(super) errors: Option<Vec<GraphqlError>>,
}

#[derive(Deserialize)]
pub(super) struct GraphqlError {
	pub(super) message: String,
	pub(super) extensions: Option<Value>,
}

#[derive(Serialize)]
pub(super) struct IssuesWithLabelVariables {
	#[serde(rename = "labelName")]
	pub(super) label_name: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) after: Option<String>,
}

#[derive(Serialize)]
pub(super) struct IssueByIdentifierVariables {
	#[serde(rename = "issueIdentifier")]
	pub(super) issue_identifier: String,
}

#[derive(Serialize)]
pub(super) struct IssuesByIdsVariables {
	#[serde(rename = "issueIds")]
	pub(super) issue_ids: Vec<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) after: Option<String>,
}

#[derive(Serialize)]
pub(super) struct IssueBlockersVariables {
	#[serde(rename = "issueId")]
	pub(super) issue_id: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) after: Option<String>,
}

#[derive(Serialize)]
pub(super) struct IssueCommentsVariables {
	#[serde(rename = "issueId")]
	pub(super) issue_id: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) after: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct IssueConnectionData {
	pub(super) issues: IssueConnection,
}

#[derive(Deserialize)]
pub(super) struct IssueByIdentifierData {
	pub(super) issue: Option<LinearIssue>,
}

#[derive(Deserialize)]
pub(super) struct IssueBlockersData {
	pub(super) issues: IssueBlockerConnection,
}

#[derive(Deserialize)]
pub(super) struct IssueCommentsData {
	pub(super) issue: Option<LinearIssueComments>,
}

#[derive(Deserialize)]
pub(super) struct IssueConnection {
	pub(super) nodes: Vec<LinearIssue>,
	#[serde(rename = "pageInfo")]
	pub(super) page_info: PageInfo,
}

#[derive(Deserialize)]
pub(super) struct IssueBlockerConnection {
	pub(super) nodes: Vec<LinearIssueBlockerPage>,
}

#[derive(Deserialize)]
pub(super) struct LinearIssueBlockerPage {
	#[serde(rename = "inverseRelations")]
	pub(super) inverse_relations: IssueRelationConnection,
}

#[derive(Deserialize)]
pub(super) struct LinearIssueComments {
	pub(super) comments: CommentConnection,
}

#[derive(Deserialize)]
pub(super) struct CommentConnection {
	pub(super) nodes: Vec<LinearComment>,
	#[serde(rename = "pageInfo")]
	pub(super) page_info: PageInfo,
}

#[derive(Deserialize)]
pub(super) struct LinearComment {
	pub(super) body: String,
	#[serde(rename = "createdAt")]
	pub(super) created_at: String,
}

#[derive(Clone, Deserialize)]
pub(super) struct PageInfo {
	#[serde(rename = "hasNextPage")]
	pub(super) has_next_page: bool,
	#[serde(rename = "endCursor")]
	pub(super) end_cursor: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct LinearIssue {
	pub(super) id: String,
	pub(super) identifier: String,
	pub(super) title: String,
	pub(super) creator: Option<LinearUser>,
	pub(super) description: Option<String>,
	pub(super) priority: Option<i64>,
	#[serde(rename = "createdAt")]
	pub(super) created_at: String,
	#[serde(rename = "updatedAt")]
	pub(super) updated_at: String,
	pub(super) state: LinearState,
	pub(super) team: LinearTeam,
	pub(super) labels: LabelConnection,
	#[serde(rename = "inverseRelations")]
	pub(super) inverse_relations: IssueRelationConnection,
}

#[derive(Deserialize)]
pub(super) struct LinearTeam {
	pub(super) id: String,
	pub(super) name: String,
	pub(super) states: StateConnection,
	pub(super) labels: LabelConnection,
}

#[derive(Deserialize)]
pub(super) struct LinearUser {
	#[serde(rename = "displayName")]
	pub(super) display_name: Option<String>,
	pub(super) name: Option<String>,
	pub(super) email: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct StateConnection {
	pub(super) nodes: Vec<LinearState>,
}

#[derive(Deserialize)]
pub(super) struct LabelConnection {
	pub(super) nodes: Vec<LinearLabel>,
	#[serde(rename = "pageInfo")]
	pub(super) page_info: Option<PageInfo>,
}

#[derive(Deserialize)]
pub(super) struct IssueRelationConnection {
	pub(super) nodes: Vec<LinearIssueRelation>,
	#[serde(rename = "pageInfo")]
	pub(super) page_info: PageInfo,
}

#[derive(Deserialize)]
pub(super) struct LinearIssueRelation {
	#[serde(rename = "type")]
	pub(super) relation_type: String,
	pub(super) issue: LinearRelatedIssue,
}

#[derive(Deserialize)]
pub(super) struct LinearRelatedIssue {
	pub(super) id: String,
	pub(super) identifier: String,
	pub(super) state: LinearState,
}

#[derive(Deserialize)]
pub(super) struct LinearState {
	pub(super) id: String,
	pub(super) name: String,
}

#[derive(Deserialize)]
pub(super) struct LinearLabel {
	pub(super) id: String,
	pub(super) name: String,
}

#[derive(Serialize)]
pub(super) struct IssueUpdateVariables<'a> {
	pub(super) id: &'a str,
	pub(super) input: IssueUpdateInput,
}

#[derive(Serialize)]
pub(super) struct IssueUpdateInput {
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) title: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) description: Option<String>,
	#[serde(rename = "stateId", skip_serializing_if = "Option::is_none")]
	pub(super) state_id: Option<String>,
	#[serde(rename = "labelIds", skip_serializing_if = "Option::is_none")]
	pub(super) label_ids: Option<Vec<String>>,
	#[serde(rename = "addedLabelIds", skip_serializing_if = "Option::is_none")]
	pub(super) added_label_ids: Option<Vec<String>>,
	#[serde(rename = "removedLabelIds", skip_serializing_if = "Option::is_none")]
	pub(super) removed_label_ids: Option<Vec<String>>,
}

#[derive(Deserialize)]
pub(super) struct IssueUpdateData {
	#[serde(rename = "issueUpdate")]
	pub(super) issue_update: MutationSuccess,
}

#[derive(Deserialize)]
pub(super) struct IssueUpdateWithIssueData {
	#[serde(rename = "issueUpdate")]
	pub(super) issue_update: IssueMutationWithIssue,
}

#[derive(Serialize)]
pub(super) struct IssueCreateVariables {
	pub(super) input: IssueCreateInput,
}

#[derive(Serialize)]
pub(super) struct IssueCreateInput {
	#[serde(rename = "teamId")]
	pub(super) team_id: String,
	pub(super) title: String,
	pub(super) description: String,
	#[serde(rename = "stateId", skip_serializing_if = "Option::is_none")]
	pub(super) state_id: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct IssueCreateData {
	#[serde(rename = "issueCreate")]
	pub(super) issue_create: IssueMutationWithIssue,
}

#[derive(Deserialize)]
pub(super) struct IssueMutationWithIssue {
	pub(super) success: bool,
	pub(super) issue: Option<LinearIssue>,
}

#[derive(Serialize)]
pub(super) struct IssueArchiveVariables<'a> {
	pub(super) id: &'a str,
	pub(super) trash: bool,
}

#[derive(Deserialize)]
pub(super) struct IssueArchiveData {
	#[serde(rename = "issueArchive")]
	pub(super) issue_archive: MutationSuccess,
}

#[derive(Deserialize)]
pub(super) struct MutationSuccess {
	pub(super) success: bool,
}

#[derive(Serialize)]
pub(super) struct TeamLabelByNameVariables {
	#[serde(rename = "teamId")]
	pub(super) team_id: String,
	#[serde(rename = "labelName")]
	pub(super) label_name: String,
}

#[derive(Deserialize)]
pub(super) struct TeamLabelByNameData {
	#[serde(rename = "issueLabels")]
	pub(super) issue_labels: LabelConnection,
}

#[derive(Serialize)]
pub(super) struct CommentCreateVariables {
	pub(super) input: CommentCreateInput,
}

#[derive(Serialize)]
pub(super) struct CommentCreateInput {
	pub(super) body: String,
	#[serde(rename = "issueId")]
	pub(super) issue_id: String,
}

#[derive(Deserialize)]
pub(super) struct CommentCreateData {
	#[serde(rename = "commentCreate")]
	pub(super) comment_create: MutationSuccess,
}
