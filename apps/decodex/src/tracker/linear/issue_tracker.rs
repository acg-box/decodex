mod mutations;
mod queries;

use crate::{
	prelude::Result,
	tracker::{
		IssueTracker, TrackerComment, TrackerIssue, TrackerIssueBriefUpdate, TrackerIssueCreate,
		linear::LinearClient,
	},
};

impl IssueTracker for LinearClient {
	fn list_issues_with_label(&self, label_name: &str) -> Result<Vec<TrackerIssue>> {
		queries::list_issues_with_label(self, label_name)
	}

	fn find_team_label_id(&self, team_id: &str, label_name: &str) -> Result<Option<String>> {
		queries::find_team_label_id(self, team_id, label_name)
	}

	fn get_issue_by_identifier(&self, issue_identifier: &str) -> Result<Option<TrackerIssue>> {
		queries::get_issue_by_identifier(self, issue_identifier)
	}

	fn refresh_issues(&self, issue_ids: &[String]) -> Result<Vec<TrackerIssue>> {
		queries::refresh_issues(self, issue_ids)
	}

	fn list_comments(&self, issue_id: &str) -> Result<Vec<TrackerComment>> {
		self.collect_issue_comments(issue_id)
	}

	fn update_issue_state(&self, issue_id: &str, state_id: &str) -> Result<()> {
		mutations::update_issue_state(self, issue_id, state_id)
	}

	fn add_issue_labels(&self, issue_id: &str, label_ids: &[String]) -> Result<()> {
		mutations::add_issue_labels(self, issue_id, label_ids)
	}

	fn remove_issue_labels(&self, issue_id: &str, label_ids: &[String]) -> Result<()> {
		mutations::remove_issue_labels(self, issue_id, label_ids)
	}

	fn create_issue(&self, request: &TrackerIssueCreate) -> Result<TrackerIssue> {
		mutations::create_issue(self, request)
	}

	fn update_issue_brief(
		&self,
		issue_id: &str,
		request: &TrackerIssueBriefUpdate,
	) -> Result<TrackerIssue> {
		mutations::update_issue_brief(self, issue_id, request)
	}

	fn create_comment(&self, issue_id: &str, body: &str) -> Result<()> {
		mutations::create_comment(self, issue_id, body)
	}
}
