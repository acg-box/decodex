use std::{cell::RefCell, collections::HashMap};

use crate::{
	prelude::{Result, eyre},
	tracker::{IssueTracker, TrackerIssue},
};

pub(super) struct TestTracker {
	pub(super) issues_by_label: HashMap<String, Vec<TrackerIssue>>,
	pub(super) comments: RefCell<Vec<String>>,
	pub(super) state_updates: RefCell<Vec<Vec<String>>>,
	pub(super) label_removals: RefCell<Vec<Vec<String>>>,
	pub(super) label_removal_error: Option<String>,
}
impl TestTracker {
	pub(super) fn new() -> Self {
		Self {
			issues_by_label: HashMap::new(),
			comments: RefCell::new(Vec::new()),
			state_updates: RefCell::new(Vec::new()),
			label_removals: RefCell::new(Vec::new()),
			label_removal_error: None,
		}
	}

	pub(super) fn with_label_issues(mut self, label_name: &str, issues: Vec<TrackerIssue>) -> Self {
		self.issues_by_label.insert(label_name.to_owned(), issues);

		self
	}

	pub(super) fn with_label_removal_error(mut self, message: &str) -> Self {
		self.label_removal_error = Some(message.to_owned());

		self
	}
}

impl IssueTracker for TestTracker {
	fn list_issues_with_label(&self, label_name: &str) -> Result<Vec<TrackerIssue>> {
		Ok(self.issues_by_label.get(label_name).cloned().unwrap_or_default())
	}

	fn find_team_label_id(&self, _team_id: &str, _label_name: &str) -> Result<Option<String>> {
		Ok(None)
	}

	fn get_issue_by_identifier(&self, _issue_identifier: &str) -> Result<Option<TrackerIssue>> {
		Ok(None)
	}

	fn refresh_issues(&self, _issue_ids: &[String]) -> Result<Vec<TrackerIssue>> {
		Ok(Vec::new())
	}

	fn list_comments(&self, _issue_id: &str) -> Result<Vec<crate::tracker::TrackerComment>> {
		Ok(self
			.comments
			.borrow()
			.iter()
			.map(|body| crate::tracker::TrackerComment {
				body: body.clone(),
				created_at: String::from("2026-06-02T00:00:00Z"),
			})
			.collect())
	}

	fn update_issue_state(&self, issue_id: &str, state_id: &str) -> Result<()> {
		self.state_updates.borrow_mut().push(vec![issue_id.to_owned(), state_id.to_owned()]);

		Ok(())
	}

	fn add_issue_labels(&self, _issue_id: &str, _label_ids: &[String]) -> Result<()> {
		Ok(())
	}

	fn remove_issue_labels(&self, _issue_id: &str, label_ids: &[String]) -> Result<()> {
		self.label_removals.borrow_mut().push(label_ids.to_vec());

		if let Some(message) = self.label_removal_error.as_ref() {
			eyre::bail!("{message}");
		}

		Ok(())
	}

	fn create_comment(&self, _issue_id: &str, body: &str) -> Result<()> {
		self.comments.borrow_mut().push(body.to_owned());

		Ok(())
	}
}
