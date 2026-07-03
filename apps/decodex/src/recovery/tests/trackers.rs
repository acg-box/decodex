use std::cell::RefCell;

use crate::{
	prelude::{Result, eyre},
	tracker::{IssueTracker, TrackerComment, TrackerIssue, TrackerLabel},
};

pub(in crate::recovery::tests) struct GhostLaneTestTracker {
	pub(in crate::recovery::tests) issues: Vec<TrackerIssue>,
	pub(in crate::recovery::tests) refresh_error: Option<String>,
	pub(in crate::recovery::tests) identifier_error: Option<String>,
	pub(in crate::recovery::tests) remove_error: Option<String>,
	pub(in crate::recovery::tests) comments: Vec<TrackerComment>,
	pub(in crate::recovery::tests) refresh_queries: RefCell<Vec<Vec<String>>>,
	pub(in crate::recovery::tests) label_removals: RefCell<Vec<(String, Vec<String>)>>,
	pub(in crate::recovery::tests) state_updates: RefCell<Vec<(String, String)>>,
}
impl GhostLaneTestTracker {
	pub(in crate::recovery::tests) fn missing() -> Self {
		Self {
			issues: Vec::new(),
			refresh_error: None,
			identifier_error: None,
			remove_error: None,
			comments: Vec::new(),
			refresh_queries: RefCell::new(Vec::new()),
			label_removals: RefCell::new(Vec::new()),
			state_updates: RefCell::new(Vec::new()),
		}
	}

	pub(in crate::recovery::tests) fn with_issues(issues: Vec<TrackerIssue>) -> Self {
		Self {
			issues,
			refresh_error: None,
			identifier_error: None,
			remove_error: None,
			comments: Vec::new(),
			refresh_queries: RefCell::new(Vec::new()),
			label_removals: RefCell::new(Vec::new()),
			state_updates: RefCell::new(Vec::new()),
		}
	}

	pub(in crate::recovery::tests) fn with_comments(
		mut self,
		comments: Vec<TrackerComment>,
	) -> Self {
		self.comments = comments;

		self
	}

	pub(in crate::recovery::tests) fn remove_error(mut self, message: &str) -> Self {
		self.remove_error = Some(message.to_owned());

		self
	}

	pub(in crate::recovery::tests) fn refresh_error(message: &str) -> Self {
		Self {
			issues: Vec::new(),
			refresh_error: Some(message.to_owned()),
			identifier_error: None,
			remove_error: None,
			comments: Vec::new(),
			refresh_queries: RefCell::new(Vec::new()),
			label_removals: RefCell::new(Vec::new()),
			state_updates: RefCell::new(Vec::new()),
		}
	}

	pub(in crate::recovery::tests) fn identifier_error(message: &str) -> Self {
		Self {
			issues: Vec::new(),
			refresh_error: None,
			identifier_error: Some(message.to_owned()),
			remove_error: None,
			comments: Vec::new(),
			refresh_queries: RefCell::new(Vec::new()),
			label_removals: RefCell::new(Vec::new()),
			state_updates: RefCell::new(Vec::new()),
		}
	}
}

impl IssueTracker for GhostLaneTestTracker {
	fn list_issues_with_label(&self, label_name: &str) -> Result<Vec<TrackerIssue>> {
		Ok(self.issues.iter().filter(|issue| issue.has_label(label_name)).cloned().collect())
	}

	fn find_team_label_id(&self, team_id: &str, label_name: &str) -> Result<Option<String>> {
		Ok(self
			.issues
			.iter()
			.find(|issue| issue.team.id == team_id)
			.and_then(|issue| issue.label_id_for_name(label_name).map(ToOwned::to_owned)))
	}

	fn get_issue_by_identifier(&self, issue_identifier: &str) -> Result<Option<TrackerIssue>> {
		if let Some(message) = &self.identifier_error {
			return Err(eyre::eyre!(message.clone()));
		}

		Ok(self
			.issues
			.iter()
			.find(|issue| issue.identifier.eq_ignore_ascii_case(issue_identifier))
			.cloned())
	}

	fn refresh_issues(&self, issue_ids: &[String]) -> Result<Vec<TrackerIssue>> {
		self.refresh_queries.borrow_mut().push(issue_ids.to_vec());

		if let Some(message) = &self.refresh_error {
			return Err(eyre::eyre!(message.clone()));
		}

		Ok(self
			.issues
			.iter()
			.filter(|issue| issue_ids.iter().any(|issue_id| issue_id == &issue.id))
			.cloned()
			.collect())
	}

	fn list_comments(&self, _issue_id: &str) -> Result<Vec<TrackerComment>> {
		Ok(self.comments.clone())
	}

	fn update_issue_state(&self, issue_id: &str, state_id: &str) -> Result<()> {
		self.state_updates.borrow_mut().push((issue_id.to_owned(), state_id.to_owned()));

		Ok(())
	}

	fn add_issue_labels(&self, _issue_id: &str, _label_ids: &[String]) -> Result<()> {
		Ok(())
	}

	fn remove_issue_labels(&self, issue_id: &str, label_ids: &[String]) -> Result<()> {
		self.label_removals.borrow_mut().push((issue_id.to_owned(), label_ids.to_vec()));

		if let Some(message) = &self.remove_error {
			return Err(eyre::eyre!(message.clone()));
		}

		Ok(())
	}

	fn create_comment(&self, _issue_id: &str, _body: &str) -> Result<()> {
		Ok(())
	}
}

pub(in crate::recovery::tests) struct FinalNeedsAttentionTracker {
	issue: TrackerIssue,
	needs_attention_label: String,
	get_issue_calls: RefCell<usize>,
	pub(in crate::recovery::tests) label_removals: RefCell<Vec<(String, Vec<String>)>>,
}
impl FinalNeedsAttentionTracker {
	pub(in crate::recovery::tests) fn new(
		issue: TrackerIssue,
		needs_attention_label: String,
	) -> Self {
		Self {
			issue,
			needs_attention_label,
			get_issue_calls: RefCell::new(0),
			label_removals: RefCell::new(Vec::new()),
		}
	}

	fn issue_for_call(&self, call_count: usize) -> TrackerIssue {
		let mut issue = self.issue.clone();

		if call_count >= 3 {
			let label = TrackerLabel {
				id: format!("label-{}", self.needs_attention_label.replace(':', "-")),
				name: self.needs_attention_label.clone(),
			};

			if !issue.team.labels.iter().any(|candidate| candidate.name == label.name) {
				issue.team.labels.push(label.clone());
			}
			if !issue.labels.iter().any(|candidate| candidate.name == label.name) {
				issue.labels.push(label);
			}
		}

		issue
	}
}

impl IssueTracker for FinalNeedsAttentionTracker {
	fn list_issues_with_label(&self, label_name: &str) -> Result<Vec<TrackerIssue>> {
		let issue = self.issue_for_call(*self.get_issue_calls.borrow());

		Ok(issue.has_label(label_name).then_some(issue).into_iter().collect())
	}

	fn find_team_label_id(&self, _team_id: &str, label_name: &str) -> Result<Option<String>> {
		Ok(Some(format!("label-{}", label_name.replace(':', "-"))))
	}

	fn get_issue_by_identifier(&self, issue_identifier: &str) -> Result<Option<TrackerIssue>> {
		let mut calls = self.get_issue_calls.borrow_mut();

		*calls += 1;

		let issue = self.issue_for_call(*calls);

		Ok((issue.identifier == issue_identifier).then_some(issue))
	}

	fn refresh_issues(&self, issue_ids: &[String]) -> Result<Vec<TrackerIssue>> {
		let issue = self.issue_for_call(*self.get_issue_calls.borrow());

		Ok(issue_ids
			.iter()
			.any(|issue_id| issue_id == &issue.id)
			.then_some(issue)
			.into_iter()
			.collect())
	}

	fn list_comments(&self, _issue_id: &str) -> Result<Vec<TrackerComment>> {
		Ok(Vec::new())
	}

	fn update_issue_state(&self, _issue_id: &str, _state_id: &str) -> Result<()> {
		Ok(())
	}

	fn add_issue_labels(&self, _issue_id: &str, _label_ids: &[String]) -> Result<()> {
		Ok(())
	}

	fn remove_issue_labels(&self, issue_id: &str, label_ids: &[String]) -> Result<()> {
		self.label_removals.borrow_mut().push((issue_id.to_owned(), label_ids.to_vec()));

		Ok(())
	}

	fn create_comment(&self, _issue_id: &str, _body: &str) -> Result<()> {
		Ok(())
	}
}
