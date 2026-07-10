use serde::{Deserialize, Serialize};

use crate::tracker::linear::schema::PageInfo;

#[derive(Serialize)]
pub(in crate::tracker::linear) struct IssuesWithLabelVariables {
	#[serde(rename = "labelName")]
	pub(in crate::tracker::linear) label_name: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::tracker::linear) after: Option<String>,
}

#[derive(Serialize)]
pub(in crate::tracker::linear) struct IssueByIdentifierVariables {
	#[serde(rename = "issueIdentifier")]
	pub(in crate::tracker::linear) issue_identifier: String,
}

#[derive(Serialize)]
pub(in crate::tracker::linear) struct IssuesByIdsVariables {
	#[serde(rename = "issueIds")]
	pub(in crate::tracker::linear) issue_ids: Vec<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::tracker::linear) after: Option<String>,
}

#[derive(Serialize)]
pub(in crate::tracker::linear) struct IssueBlockersVariables {
	#[serde(rename = "issueId")]
	pub(in crate::tracker::linear) issue_id: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::tracker::linear) after: Option<String>,
}

#[derive(Serialize)]
pub(in crate::tracker::linear) struct IssueCommentsVariables {
	#[serde(rename = "issueId")]
	pub(in crate::tracker::linear) issue_id: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::tracker::linear) after: Option<String>,
}

#[derive(Serialize)]
pub(in crate::tracker::linear) struct IssueRelationsVariables {
	#[serde(rename = "issueId")]
	pub(in crate::tracker::linear) issue_id: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::tracker::linear) after: Option<String>,
}

#[derive(Deserialize)]
pub(in crate::tracker::linear) struct IssueConnectionData {
	pub(in crate::tracker::linear) issues: IssueConnection,
}

#[derive(Deserialize)]
pub(in crate::tracker::linear) struct IssueByIdentifierData {
	pub(in crate::tracker::linear) issue: Option<LinearIssue>,
}

#[derive(Deserialize)]
pub(in crate::tracker::linear) struct IssueBlockersData {
	pub(in crate::tracker::linear) issues: IssueBlockerConnection,
}

#[derive(Deserialize)]
pub(in crate::tracker::linear) struct IssueCommentsData {
	pub(in crate::tracker::linear) issue: Option<LinearIssueComments>,
}

#[derive(Deserialize)]
pub(in crate::tracker::linear) struct IssueRelationsData {
	pub(in crate::tracker::linear) issue: Option<LinearIssueRelations>,
}

#[derive(Deserialize)]
pub(in crate::tracker::linear) struct IssueInverseRelationsData {
	pub(in crate::tracker::linear) issue: Option<LinearIssueInverseRelations>,
}

#[derive(Deserialize)]
pub(in crate::tracker::linear) struct IssueConnection {
	pub(in crate::tracker::linear) nodes: Vec<LinearIssue>,
	#[serde(rename = "pageInfo")]
	pub(in crate::tracker::linear) page_info: PageInfo,
}

#[derive(Deserialize)]
pub(in crate::tracker::linear) struct IssueBlockerConnection {
	pub(in crate::tracker::linear) nodes: Vec<LinearIssueBlockerPage>,
}

#[derive(Deserialize)]
pub(in crate::tracker::linear) struct LinearIssueBlockerPage {
	#[serde(rename = "inverseRelations")]
	pub(in crate::tracker::linear) inverse_relations: IssueRelationConnection,
}

#[derive(Deserialize)]
pub(in crate::tracker::linear) struct LinearIssueComments {
	pub(in crate::tracker::linear) comments: CommentConnection,
}

#[derive(Deserialize)]
pub(in crate::tracker::linear) struct LinearIssueRelations {
	pub(in crate::tracker::linear) relations: ExplicitIssueRelationConnection,
}

#[derive(Deserialize)]
pub(in crate::tracker::linear) struct LinearIssueInverseRelations {
	#[serde(rename = "inverseRelations")]
	pub(in crate::tracker::linear) inverse_relations: ExplicitIssueRelationConnection,
}

#[derive(Deserialize)]
pub(in crate::tracker::linear) struct CommentConnection {
	pub(in crate::tracker::linear) nodes: Vec<LinearComment>,
	#[serde(rename = "pageInfo")]
	pub(in crate::tracker::linear) page_info: PageInfo,
}

#[derive(Deserialize)]
pub(in crate::tracker::linear) struct LinearComment {
	pub(in crate::tracker::linear) body: String,
	#[serde(rename = "createdAt")]
	pub(in crate::tracker::linear) created_at: String,
}

#[derive(Deserialize)]
pub(in crate::tracker::linear) struct LinearIssue {
	pub(in crate::tracker::linear) id: String,
	pub(in crate::tracker::linear) identifier: String,
	pub(in crate::tracker::linear) title: String,
	pub(in crate::tracker::linear) creator: Option<LinearUser>,
	pub(in crate::tracker::linear) description: Option<String>,
	pub(in crate::tracker::linear) priority: Option<i64>,
	#[serde(rename = "createdAt")]
	pub(in crate::tracker::linear) created_at: String,
	#[serde(rename = "updatedAt")]
	pub(in crate::tracker::linear) updated_at: String,
	pub(in crate::tracker::linear) state: LinearState,
	pub(in crate::tracker::linear) team: LinearTeam,
	pub(in crate::tracker::linear) labels: LabelConnection,
	#[serde(rename = "inverseRelations")]
	pub(in crate::tracker::linear) inverse_relations: IssueRelationConnection,
}

#[derive(Deserialize)]
pub(in crate::tracker::linear) struct LinearTeam {
	pub(in crate::tracker::linear) id: String,
	pub(in crate::tracker::linear) name: String,
	pub(in crate::tracker::linear) states: StateConnection,
	pub(in crate::tracker::linear) labels: LabelConnection,
}

#[derive(Deserialize)]
pub(in crate::tracker::linear) struct LinearUser {
	#[serde(rename = "displayName")]
	pub(in crate::tracker::linear) display_name: Option<String>,
	pub(in crate::tracker::linear) name: Option<String>,
	pub(in crate::tracker::linear) email: Option<String>,
}

#[derive(Deserialize)]
pub(in crate::tracker::linear) struct StateConnection {
	pub(in crate::tracker::linear) nodes: Vec<LinearState>,
}

#[derive(Deserialize)]
pub(in crate::tracker::linear) struct LabelConnection {
	pub(in crate::tracker::linear) nodes: Vec<LinearLabel>,
	#[serde(rename = "pageInfo")]
	pub(in crate::tracker::linear) page_info: Option<PageInfo>,
}

#[derive(Deserialize)]
pub(in crate::tracker::linear) struct IssueRelationConnection {
	pub(in crate::tracker::linear) nodes: Vec<LinearIssueRelation>,
	#[serde(rename = "pageInfo")]
	pub(in crate::tracker::linear) page_info: PageInfo,
}

#[derive(Deserialize)]
pub(in crate::tracker::linear) struct ExplicitIssueRelationConnection {
	pub(in crate::tracker::linear) nodes: Vec<ExplicitIssueRelation>,
	#[serde(rename = "pageInfo")]
	pub(in crate::tracker::linear) page_info: PageInfo,
}

#[derive(Deserialize)]
pub(in crate::tracker::linear) struct ExplicitIssueRelation {
	pub(in crate::tracker::linear) issue: ExplicitRelatedIssue,
	#[serde(rename = "relatedIssue")]
	pub(in crate::tracker::linear) related_issue: ExplicitRelatedIssue,
}

#[derive(Deserialize)]
pub(in crate::tracker::linear) struct ExplicitRelatedIssue {
	pub(in crate::tracker::linear) id: String,
}

#[derive(Deserialize)]
pub(in crate::tracker::linear) struct LinearIssueRelation {
	#[serde(rename = "type")]
	pub(in crate::tracker::linear) relation_type: String,
	pub(in crate::tracker::linear) issue: LinearRelatedIssue,
}

#[derive(Deserialize)]
pub(in crate::tracker::linear) struct LinearRelatedIssue {
	pub(in crate::tracker::linear) id: String,
	pub(in crate::tracker::linear) identifier: String,
	pub(in crate::tracker::linear) state: LinearState,
}

#[derive(Deserialize)]
pub(in crate::tracker::linear) struct LinearState {
	pub(in crate::tracker::linear) id: String,
	pub(in crate::tracker::linear) name: String,
}

#[derive(Deserialize)]
pub(in crate::tracker::linear) struct LinearLabel {
	pub(in crate::tracker::linear) id: String,
	pub(in crate::tracker::linear) name: String,
}
