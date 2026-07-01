use crate::{
	prelude::{Result, eyre},
	tracker::{
		IssueTracker, TrackerComment, TrackerIssue, TrackerIssueBriefUpdate, TrackerIssueCreate,
		linear::{
			LinearClient, mapping,
			queries::{
				COMMENT_CREATE_MUTATION, ISSUE_BY_IDENTIFIER_QUERY, ISSUE_CREATE_MUTATION,
				ISSUE_UPDATE_BRIEF_MUTATION, ISSUE_UPDATE_MUTATION, ISSUES_BY_IDS_QUERY,
				ISSUES_WITH_LABEL_QUERY, TEAM_LABEL_BY_NAME_QUERY,
			},
			schema::{
				CommentCreateData, CommentCreateInput, CommentCreateVariables,
				IssueByIdentifierData, IssueByIdentifierVariables, IssueCreateData,
				IssueCreateInput, IssueCreateVariables, IssueUpdateData, IssueUpdateInput,
				IssueUpdateVariables, IssueUpdateWithIssueData, IssuesByIdsVariables,
				IssuesWithLabelVariables, TeamLabelByNameData, TeamLabelByNameVariables,
			},
		},
	},
};

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

		Ok(Some(mapping::map_issue(issue, blockers)))
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

		Ok(mapping::map_issue(issue, blockers))
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

		Ok(mapping::map_issue(issue, blockers))
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
