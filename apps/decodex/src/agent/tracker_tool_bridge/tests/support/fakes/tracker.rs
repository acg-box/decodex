use std::{cell::RefCell, collections::HashMap};

use crate::{
	prelude::{Result, eyre},
	tracker::{IssueTracker, TrackerComment, TrackerIssue},
};

pub(crate) struct FakeTracker {
	pub(crate) state_updates: RefCell<Vec<String>>,
	pub(crate) label_updates: RefCell<Vec<Vec<String>>>,
	pub(crate) label_additions: RefCell<Vec<Vec<String>>>,
	pub(crate) label_removals: RefCell<Vec<Vec<String>>>,
	pub(crate) comments: RefCell<Vec<String>>,
	pub(crate) issue_comments: RefCell<HashMap<String, Vec<TrackerComment>>>,
	pub(crate) refresh_snapshots: RefCell<Vec<Vec<TrackerIssue>>>,
	pub(crate) issues_by_label: RefCell<HashMap<String, Vec<TrackerIssue>>>,
	pub(crate) team_label_ids_by_name: RefCell<HashMap<(String, String), String>>,
	pub(crate) fail_state_update: RefCell<Option<String>>,
	pub(crate) fail_label_update: RefCell<Option<String>>,
	pub(crate) fail_comment: RefCell<Option<String>>,
}
impl FakeTracker {
	pub(crate) fn new() -> Self {
		Self {
			state_updates: RefCell::new(Vec::new()),
			label_updates: RefCell::new(Vec::new()),
			label_additions: RefCell::new(Vec::new()),
			label_removals: RefCell::new(Vec::new()),
			comments: RefCell::new(Vec::new()),
			issue_comments: RefCell::new(HashMap::new()),
			refresh_snapshots: RefCell::new(Vec::new()),
			issues_by_label: RefCell::new(HashMap::new()),
			team_label_ids_by_name: RefCell::new(HashMap::new()),
			fail_state_update: RefCell::new(None),
			fail_label_update: RefCell::new(None),
			fail_comment: RefCell::new(None),
		}
	}

	pub(crate) fn with_refresh_snapshots(refresh_snapshots: Vec<Vec<TrackerIssue>>) -> Self {
		let tracker = Self::new();

		tracker.refresh_snapshots.replace(refresh_snapshots);

		tracker
	}

	pub(crate) fn with_state_update_error(message: &str) -> Self {
		let tracker = Self::new();

		tracker.fail_state_update.replace(Some(message.to_owned()));

		tracker
	}

	pub(crate) fn with_label_update_error(message: &str) -> Self {
		let tracker = Self::new();

		tracker.fail_label_update.replace(Some(message.to_owned()));

		tracker
	}

	pub(crate) fn with_comment_error(message: &str) -> Self {
		let tracker = Self::new();

		tracker.fail_comment.replace(Some(message.to_owned()));

		tracker
	}

	pub(crate) fn with_label_lookup_issues(
		self,
		label_name: &str,
		issues: Vec<TrackerIssue>,
	) -> Self {
		self.issues_by_label.borrow_mut().insert(label_name.to_owned(), issues);

		self
	}

	pub(crate) fn with_team_label_lookup_id(
		self,
		team_id: &str,
		label_name: &str,
		label_id: &str,
	) -> Self {
		self.team_label_ids_by_name
			.borrow_mut()
			.insert((team_id.to_owned(), label_name.to_owned()), label_id.to_owned());

		self
	}
}

impl IssueTracker for FakeTracker {
	fn list_issues_with_label(&self, label_name: &str) -> Result<Vec<TrackerIssue>> {
		Ok(self.issues_by_label.borrow().get(label_name).cloned().unwrap_or_default())
	}

	fn find_team_label_id(&self, team_id: &str, label_name: &str) -> Result<Option<String>> {
		Ok(self
			.team_label_ids_by_name
			.borrow()
			.get(&(team_id.to_owned(), label_name.to_owned()))
			.cloned())
	}

	fn get_issue_by_identifier(&self, _issue_identifier: &str) -> Result<Option<TrackerIssue>> {
		Ok(None)
	}

	fn refresh_issues(&self, _issue_ids: &[String]) -> Result<Vec<TrackerIssue>> {
		if self.refresh_snapshots.borrow().is_empty() {
			return Ok(Vec::new());
		}

		Ok(self.refresh_snapshots.borrow_mut().remove(0))
	}

	fn list_comments(&self, issue_id: &str) -> Result<Vec<TrackerComment>> {
		Ok(self.issue_comments.borrow().get(issue_id).cloned().unwrap_or_default())
	}

	fn update_issue_state(&self, _issue_id: &str, state_id: &str) -> Result<()> {
		if let Some(message) = self.fail_state_update.borrow().as_ref() {
			return Err(eyre::eyre!(message.clone()));
		}

		self.state_updates.borrow_mut().push(state_id.to_owned());

		Ok(())
	}

	fn add_issue_labels(&self, _issue_id: &str, label_ids: &[String]) -> Result<()> {
		if let Some(message) = self.fail_label_update.borrow().as_ref() {
			return Err(eyre::eyre!(message.clone()));
		}

		self.label_additions.borrow_mut().push(label_ids.to_vec());

		Ok(())
	}

	fn remove_issue_labels(&self, _issue_id: &str, label_ids: &[String]) -> Result<()> {
		if let Some(message) = self.fail_label_update.borrow().as_ref() {
			return Err(eyre::eyre!(message.clone()));
		}

		self.label_removals.borrow_mut().push(label_ids.to_vec());

		Ok(())
	}

	fn create_comment(&self, issue_id: &str, body: &str) -> Result<()> {
		if let Some(message) = self.fail_comment.borrow().as_ref() {
			return Err(eyre::eyre!(message.clone()));
		}

		self.comments.borrow_mut().push(body.to_owned());
		self.issue_comments.borrow_mut().entry(issue_id.to_owned()).or_default().push(
			TrackerComment {
				body: body.to_owned(),
				created_at: String::from("2026-04-12T00:00:00Z"),
			},
		);

		Ok(())
	}
}
