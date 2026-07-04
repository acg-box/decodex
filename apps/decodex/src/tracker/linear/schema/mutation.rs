use serde::{Deserialize, Serialize};

use crate::tracker::linear::schema::{LinearIssue, issue::LabelConnection};

#[derive(Serialize)]
pub(in crate::tracker::linear) struct IssueUpdateVariables<'a> {
	pub(in crate::tracker::linear) id: &'a str,
	pub(in crate::tracker::linear) input: IssueUpdateInput,
}

#[derive(Serialize)]
pub(in crate::tracker::linear) struct IssueUpdateInput {
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::tracker::linear) title: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::tracker::linear) description: Option<String>,
	#[serde(rename = "stateId", skip_serializing_if = "Option::is_none")]
	pub(in crate::tracker::linear) state_id: Option<String>,
	#[serde(rename = "labelIds", skip_serializing_if = "Option::is_none")]
	pub(in crate::tracker::linear) label_ids: Option<Vec<String>>,
	#[serde(rename = "addedLabelIds", skip_serializing_if = "Option::is_none")]
	pub(in crate::tracker::linear) added_label_ids: Option<Vec<String>>,
	#[serde(rename = "removedLabelIds", skip_serializing_if = "Option::is_none")]
	pub(in crate::tracker::linear) removed_label_ids: Option<Vec<String>>,
}

#[derive(Deserialize)]
pub(in crate::tracker::linear) struct IssueUpdateData {
	#[serde(rename = "issueUpdate")]
	pub(in crate::tracker::linear) issue_update: MutationSuccess,
}

#[derive(Deserialize)]
pub(in crate::tracker::linear) struct IssueUpdateWithIssueData {
	#[serde(rename = "issueUpdate")]
	pub(in crate::tracker::linear) issue_update: IssueMutationWithIssue,
}

#[derive(Serialize)]
pub(in crate::tracker::linear) struct IssueCreateVariables {
	pub(in crate::tracker::linear) input: IssueCreateInput,
}

#[derive(Serialize)]
pub(in crate::tracker::linear) struct IssueCreateInput {
	#[serde(rename = "teamId")]
	pub(in crate::tracker::linear) team_id: String,
	pub(in crate::tracker::linear) title: String,
	pub(in crate::tracker::linear) description: String,
	#[serde(rename = "stateId", skip_serializing_if = "Option::is_none")]
	pub(in crate::tracker::linear) state_id: Option<String>,
}

#[derive(Deserialize)]
pub(in crate::tracker::linear) struct IssueCreateData {
	#[serde(rename = "issueCreate")]
	pub(in crate::tracker::linear) issue_create: IssueMutationWithIssue,
}

#[derive(Deserialize)]
pub(in crate::tracker::linear) struct IssueMutationWithIssue {
	pub(in crate::tracker::linear) success: bool,
	pub(in crate::tracker::linear) issue: Option<LinearIssue>,
}

#[derive(Serialize)]
pub(in crate::tracker::linear) struct IssueArchiveVariables<'a> {
	pub(in crate::tracker::linear) id: &'a str,
	pub(in crate::tracker::linear) trash: bool,
}

#[derive(Deserialize)]
pub(in crate::tracker::linear) struct IssueArchiveData {
	#[serde(rename = "issueArchive")]
	pub(in crate::tracker::linear) issue_archive: MutationSuccess,
}

#[derive(Deserialize)]
pub(in crate::tracker::linear) struct MutationSuccess {
	pub(in crate::tracker::linear) success: bool,
}

#[derive(Serialize)]
pub(in crate::tracker::linear) struct TeamLabelByNameVariables {
	#[serde(rename = "teamId")]
	pub(in crate::tracker::linear) team_id: String,
	#[serde(rename = "labelName")]
	pub(in crate::tracker::linear) label_name: String,
}

#[derive(Deserialize)]
pub(in crate::tracker::linear) struct TeamLabelByNameData {
	#[serde(rename = "issueLabels")]
	pub(in crate::tracker::linear) issue_labels: LabelConnection,
}

#[derive(Serialize)]
pub(in crate::tracker::linear) struct CommentCreateVariables {
	pub(in crate::tracker::linear) input: CommentCreateInput,
}

#[derive(Serialize)]
pub(in crate::tracker::linear) struct CommentCreateInput {
	pub(in crate::tracker::linear) body: String,
	#[serde(rename = "issueId")]
	pub(in crate::tracker::linear) issue_id: String,
}

#[derive(Deserialize)]
pub(in crate::tracker::linear) struct CommentCreateData {
	#[serde(rename = "commentCreate")]
	pub(in crate::tracker::linear) comment_create: MutationSuccess,
}
