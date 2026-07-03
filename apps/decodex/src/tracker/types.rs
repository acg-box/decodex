use crate::prelude::{Result, eyre};

pub(crate) trait IssueTracker {
	fn list_issues_with_label(&self, label_name: &str) -> Result<Vec<TrackerIssue>>;
	fn find_team_label_id(&self, team_id: &str, label_name: &str) -> Result<Option<String>>;
	fn get_issue_by_identifier(&self, issue_identifier: &str) -> Result<Option<TrackerIssue>>;
	fn refresh_issues(&self, issue_ids: &[String]) -> Result<Vec<TrackerIssue>>;
	fn list_comments(&self, issue_id: &str) -> Result<Vec<TrackerComment>>;
	fn update_issue_state(&self, issue_id: &str, state_id: &str) -> Result<()>;
	fn add_issue_labels(&self, issue_id: &str, label_ids: &[String]) -> Result<()>;
	fn remove_issue_labels(&self, issue_id: &str, label_ids: &[String]) -> Result<()>;
	fn create_comment(&self, issue_id: &str, body: &str) -> Result<()>;
	fn create_issue(&self, request: &TrackerIssueCreate) -> Result<TrackerIssue> {
		let _ = request;

		eyre::bail!("Issue tracker does not support creating issues.")
	}
	fn update_issue_brief(
		&self,
		issue_id: &str,
		request: &TrackerIssueBriefUpdate,
	) -> Result<TrackerIssue> {
		let _ = (issue_id, request);

		eyre::bail!("Issue tracker does not support updating issue briefs.")
	}
}

/// Public-safe normal issue creation request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TrackerIssueCreate {
	/// Linear team id that will own the issue.
	pub(crate) team_id: String,
	/// Public issue title.
	pub(crate) title: String,
	/// Natural-language public issue brief.
	pub(crate) description: String,
	/// Optional initial tracker state id.
	pub(crate) state_id: Option<String>,
}

/// Public-safe normal issue brief update request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TrackerIssueBriefUpdate {
	/// Replacement public issue title.
	pub(crate) title: String,
	/// Replacement natural-language public issue brief.
	pub(crate) description: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TrackerComment {
	pub(crate) body: String,
	pub(crate) created_at: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TrackerIssue {
	pub(crate) id: String,
	pub(crate) identifier: String,
	#[cfg(test)]
	pub(crate) project_slug: Option<String>,
	pub(crate) title: String,
	pub(crate) author: Option<String>,
	pub(crate) description: String,
	pub(crate) priority: Option<i64>,
	pub(crate) created_at: String,
	pub(crate) updated_at: String,
	pub(crate) state: TrackerState,
	pub(crate) team: TrackerTeam,
	pub(crate) labels_complete: bool,
	pub(crate) labels: Vec<TrackerLabel>,
	pub(crate) blockers: Vec<TrackerIssueBlocker>,
}
impl TrackerIssue {
	pub(crate) fn has_label(&self, label_name: &str) -> bool {
		self.labels.iter().any(|label| label.name == label_name)
	}

	pub(crate) fn state_id_for_name(&self, state_name: &str) -> Option<&str> {
		self.team
			.states
			.iter()
			.find(|state| state.name == state_name)
			.map(|state| state.id.as_str())
	}

	pub(crate) fn label_id_for_name(&self, label_name: &str) -> Option<&str> {
		self.team
			.labels
			.iter()
			.find(|label| label.name == label_name)
			.map(|label| label.id.as_str())
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TrackerIssueBlocker {
	pub(crate) id: String,
	pub(crate) identifier: String,
	pub(crate) state: TrackerState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TrackerState {
	pub(crate) id: String,
	pub(crate) name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TrackerLabel {
	pub(crate) id: String,
	pub(crate) name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TrackerTeam {
	pub(crate) id: String,
	pub(crate) name: String,
	pub(crate) states: Vec<TrackerState>,
	pub(crate) labels: Vec<TrackerLabel>,
}
