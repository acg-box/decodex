use crate::{
	prelude::{Result, eyre},
	tracker::{
		TrackerIssue, TrackerIssueBriefUpdate, TrackerIssueCreate,
		linear::{
			LinearClient, mapping,
			queries::{
				COMMENT_CREATE_MUTATION, ISSUE_CREATE_MUTATION, ISSUE_UPDATE_BRIEF_MUTATION,
				ISSUE_UPDATE_MUTATION,
			},
			schema::{
				CommentCreateData, CommentCreateInput, CommentCreateVariables, IssueCreateData,
				IssueCreateInput, IssueCreateVariables, IssueUpdateData, IssueUpdateInput,
				IssueUpdateVariables, IssueUpdateWithIssueData,
			},
		},
	},
};

pub(in crate::tracker::linear::issue_tracker) fn update_issue_state(
	client: &LinearClient,
	issue_id: &str,
	state_id: &str,
) -> Result<()> {
	let data = client.post::<_, IssueUpdateData>(
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

	ensure_issue_update_confirmed(data.issue_update.success, "state update")
}

pub(in crate::tracker::linear::issue_tracker) fn add_issue_labels(
	client: &LinearClient,
	issue_id: &str,
	label_ids: &[String],
) -> Result<()> {
	let data = client.post::<_, IssueUpdateData>(
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

	ensure_issue_update_confirmed(data.issue_update.success, "label addition")
}

pub(in crate::tracker::linear::issue_tracker) fn remove_issue_labels(
	client: &LinearClient,
	issue_id: &str,
	label_ids: &[String],
) -> Result<()> {
	let data = client.post::<_, IssueUpdateData>(
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

	ensure_issue_update_confirmed(data.issue_update.success, "label removal")
}

pub(in crate::tracker::linear::issue_tracker) fn create_issue(
	client: &LinearClient,
	request: &TrackerIssueCreate,
) -> Result<TrackerIssue> {
	let data = client.post::<_, IssueCreateData>(
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
	let blockers = client.resolve_issue_blockers(&issue)?;

	Ok(mapping::map_issue(issue, blockers))
}

pub(in crate::tracker::linear::issue_tracker) fn update_issue_brief(
	client: &LinearClient,
	issue_id: &str,
	request: &TrackerIssueBriefUpdate,
) -> Result<TrackerIssue> {
	let data = client.post::<_, IssueUpdateWithIssueData>(
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
	let blockers = client.resolve_issue_blockers(&issue)?;

	Ok(mapping::map_issue(issue, blockers))
}

pub(in crate::tracker::linear::issue_tracker) fn create_comment(
	client: &LinearClient,
	issue_id: &str,
	body: &str,
) -> Result<()> {
	let data = client.post::<_, CommentCreateData>(
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

fn ensure_issue_update_confirmed(success: bool, action: &str) -> Result<()> {
	if !success {
		eyre::bail!("Linear did not confirm the issue {action}.");
	}

	Ok(())
}
