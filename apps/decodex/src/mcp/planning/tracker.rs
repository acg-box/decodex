use crate::{
	prelude::eyre,
	tracker::{
		IssueTracker, TrackerComment, TrackerIssue, TrackerIssueBriefUpdate, TrackerIssueCreate,
	},
};

pub(super) struct McpDryRunTracker;
impl IssueTracker for McpDryRunTracker {
	fn list_issues_with_label(
		&self,
		_label_name: &str,
	) -> crate::prelude::Result<Vec<TrackerIssue>> {
		Ok(Vec::new())
	}

	fn find_team_label_id(
		&self,
		_team_id: &str,
		_label_name: &str,
	) -> crate::prelude::Result<Option<String>> {
		Ok(None)
	}

	fn get_issue_by_identifier(
		&self,
		_issue_identifier: &str,
	) -> crate::prelude::Result<Option<TrackerIssue>> {
		Ok(None)
	}

	fn refresh_issues(&self, _issue_ids: &[String]) -> crate::prelude::Result<Vec<TrackerIssue>> {
		Ok(Vec::new())
	}

	fn list_comments(&self, _issue_id: &str) -> crate::prelude::Result<Vec<TrackerComment>> {
		Ok(Vec::new())
	}

	fn update_issue_state(&self, _issue_id: &str, _state_id: &str) -> crate::prelude::Result<()> {
		eyre::bail!("MCP dry-run tracker does not mutate issue state.")
	}

	fn add_issue_labels(
		&self,
		_issue_id: &str,
		_label_ids: &[String],
	) -> crate::prelude::Result<()> {
		eyre::bail!("MCP dry-run tracker does not mutate labels.")
	}

	fn remove_issue_labels(
		&self,
		_issue_id: &str,
		_label_ids: &[String],
	) -> crate::prelude::Result<()> {
		eyre::bail!("MCP dry-run tracker does not mutate labels.")
	}

	fn create_comment(&self, _issue_id: &str, _body: &str) -> crate::prelude::Result<()> {
		eyre::bail!("MCP dry-run tracker does not create comments.")
	}

	fn create_issue(&self, _request: &TrackerIssueCreate) -> crate::prelude::Result<TrackerIssue> {
		eyre::bail!("MCP dry-run tracker does not create issues.")
	}

	fn update_issue_brief(
		&self,
		_issue_id: &str,
		_request: &TrackerIssueBriefUpdate,
	) -> crate::prelude::Result<TrackerIssue> {
		eyre::bail!("MCP dry-run tracker does not update issue briefs.")
	}
}
