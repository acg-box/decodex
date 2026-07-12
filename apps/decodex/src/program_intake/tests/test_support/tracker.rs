use std::{cell::RefCell, collections::HashMap};

use crate::{
	prelude::{Result, eyre},
	program_intake::tests::test_support,
	tracker::{
		IssueTracker, TrackerComment, TrackerIssue, TrackerIssueBlocker, TrackerIssueBriefUpdate,
		TrackerIssueCreate, TrackerLabel, TrackerState,
	},
};

pub(crate) trait TestIssueExt {
	fn with_blocker(self, identifier: &str, state: &str) -> Self;
	fn with_label(self, name: &str) -> Self;
}

#[derive(Default)]
pub(crate) struct FakeTracker {
	issues: RefCell<HashMap<String, TrackerIssue>>,
	next_issue_number: RefCell<usize>,
	created_issues: RefCell<Vec<TrackerIssue>>,
	updated_issues: RefCell<Vec<TrackerIssue>>,
	label_query_snapshots: RefCell<HashMap<String, Vec<Vec<TrackerIssue>>>>,
	fail_create_after_successes: RefCell<Option<usize>>,
	fail_update_after_successes: RefCell<Option<usize>>,
}
impl FakeTracker {
	pub(crate) fn with_issues(self, issues: impl IntoIterator<Item = TrackerIssue>) -> Self {
		for issue in issues {
			self.issues.borrow_mut().insert(issue.identifier.clone(), issue);
		}

		self
	}

	pub(crate) fn with_create_failure_after_successes(self, successes: usize) -> Self {
		*self.fail_create_after_successes.borrow_mut() = Some(successes);

		self
	}

	pub(crate) fn with_update_failure_after_successes(self, successes: usize) -> Self {
		*self.fail_update_after_successes.borrow_mut() = Some(successes);

		self
	}

	pub(crate) fn with_label_query_snapshots(
		self,
		label_name: &str,
		snapshots: Vec<Vec<TrackerIssue>>,
	) -> Self {
		self.label_query_snapshots.borrow_mut().insert(label_name.to_owned(), snapshots);
		self
	}

	pub(crate) fn created_issue_count(&self) -> usize {
		self.created_issues.borrow().len()
	}

	pub(crate) fn updated_issue_count(&self) -> usize {
		self.updated_issues.borrow().len()
	}

	pub(crate) fn generated_issue_identifier(&self, index: usize) -> String {
		format!("XY-G{}", index + 1)
	}
}

impl IssueTracker for FakeTracker {
	fn list_issues_with_label(&self, label_name: &str) -> Result<Vec<TrackerIssue>> {
		if let Some(snapshots) = self.label_query_snapshots.borrow_mut().get_mut(label_name)
			&& !snapshots.is_empty()
		{
			return Ok(snapshots.remove(0));
		}
		Ok(self
			.issues
			.borrow()
			.values()
			.filter(|issue| issue.has_label(label_name))
			.cloned()
			.collect())
	}

	fn find_team_label_id(&self, _team_id: &str, label_name: &str) -> Result<Option<String>> {
		Ok(Some(format!("label-{label_name}")))
	}

	fn get_issue_by_identifier(&self, issue_identifier: &str) -> Result<Option<TrackerIssue>> {
		Ok(self.issues.borrow().get(issue_identifier).cloned())
	}

	fn refresh_issues(&self, _issue_ids: &[String]) -> Result<Vec<TrackerIssue>> {
		Ok(Vec::new())
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

	fn remove_issue_labels(&self, _issue_id: &str, _label_ids: &[String]) -> Result<()> {
		Ok(())
	}

	fn create_comment(&self, _issue_id: &str, _body: &str) -> Result<()> {
		Ok(())
	}

	fn create_issue(&self, request: &TrackerIssueCreate) -> Result<TrackerIssue> {
		if let Some(success_limit) = *self.fail_create_after_successes.borrow()
			&& self.created_issues.borrow().len() >= success_limit
		{
			eyre::bail!("injected create failure after {success_limit} successes");
		}

		let identifier = loop {
			let mut next_issue_number = self.next_issue_number.borrow_mut();

			*next_issue_number += 1;

			let candidate = self.generated_issue_identifier(*next_issue_number - 1);

			if !self.issues.borrow().contains_key(&candidate) {
				break candidate;
			}
		};
		let state_name = request
			.state_id
			.as_deref()
			.and_then(|state_id| state_id.strip_prefix("state-"))
			.unwrap_or("Todo");
		let mut issue = test_support::issue(&identifier, state_name);

		issue.id = format!("id-{identifier}");

		issue.title.clone_from(&request.title);
		issue.description.clone_from(&request.description);
		issue.team.id.clone_from(&request.team_id);
		self.issues.borrow_mut().insert(identifier, issue.clone());
		self.created_issues.borrow_mut().push(issue.clone());

		Ok(issue)
	}

	fn update_issue_brief(
		&self,
		issue_id: &str,
		request: &TrackerIssueBriefUpdate,
	) -> Result<TrackerIssue> {
		if let Some(success_limit) = *self.fail_update_after_successes.borrow()
			&& self.updated_issues.borrow().len() >= success_limit
		{
			eyre::bail!("injected update failure after {success_limit} successes");
		}

		let mut issues = self.issues.borrow_mut();
		let issue = issues
			.values_mut()
			.find(|issue| issue.id == issue_id)
			.ok_or_else(|| eyre::eyre!("issue `{issue_id}` not found"))?;

		issue.title.clone_from(&request.title);
		issue.description.clone_from(&request.description);

		let issue = issue.clone();

		self.updated_issues.borrow_mut().push(issue.clone());

		Ok(issue)
	}
}

impl TestIssueExt for TrackerIssue {
	fn with_blocker(mut self, identifier: &str, state: &str) -> Self {
		self.blockers.push(TrackerIssueBlocker {
			id: format!("id-{identifier}"),
			identifier: identifier.to_owned(),
			state: TrackerState { id: format!("state-{state}"), name: state.to_owned() },
		});

		self
	}

	fn with_label(mut self, name: &str) -> Self {
		self.labels.push(TrackerLabel { id: format!("label-{name}"), name: name.to_owned() });

		self
	}
}
