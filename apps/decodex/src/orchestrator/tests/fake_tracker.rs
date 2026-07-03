use crate::orchestrator::tests::{
	self, HashMap, IssueTracker, RefCell, Report, Result, TrackerComment, TrackerIssue,
	TrackerIssueCreate,
};

pub(super) struct FakeTracker {
	pub(super) listed_issues: Vec<TrackerIssue>,
	pub(super) identifier_lookup_issues: Option<Vec<TrackerIssue>>,
	pub(super) issues_by_label: HashMap<String, Vec<TrackerIssue>>,
	pub(super) team_label_ids_by_name: HashMap<(String, String), String>,
	pub(super) identifier_queries: RefCell<Vec<String>>,
	pub(super) refresh_snapshots: RefCell<Vec<Vec<TrackerIssue>>>,
	pub(super) refresh_error: RefCell<Option<String>>,
	pub(super) refresh_queries: RefCell<Vec<Vec<String>>>,
	pub(super) label_queries: RefCell<Vec<String>>,
	pub(super) comment_queries: RefCell<Vec<String>>,
	pub(super) comments: RefCell<Vec<String>>,
	pub(super) issue_comments: RefCell<HashMap<String, Vec<TrackerComment>>>,
	pub(super) state_updates: RefCell<Vec<(String, String)>>,
	pub(super) label_updates: RefCell<Vec<(String, Vec<String>)>>,
	pub(super) label_additions: RefCell<Vec<(String, Vec<String>)>>,
	pub(super) label_removals: RefCell<Vec<(String, Vec<String>)>>,
	pub(super) next_created_issue_number: RefCell<usize>,
}
impl FakeTracker {
	pub(super) fn new(issues: Vec<TrackerIssue>) -> Self {
		Self::with_refresh_snapshots_and_project(issues.clone(), vec![issues], true)
	}

	pub(super) fn with_refresh_snapshots(
		listed_issues: Vec<TrackerIssue>,
		refresh_snapshots: Vec<Vec<TrackerIssue>>,
	) -> Self {
		Self::with_refresh_snapshots_and_project(listed_issues, refresh_snapshots, true)
	}

	pub(super) fn with_refresh_snapshots_and_project(
		listed_issues: Vec<TrackerIssue>,
		refresh_snapshots: Vec<Vec<TrackerIssue>>,
		_project_exists: bool,
	) -> Self {
		Self {
			listed_issues,
			identifier_lookup_issues: None,
			issues_by_label: HashMap::new(),
			team_label_ids_by_name: HashMap::new(),
			identifier_queries: RefCell::new(Vec::new()),
			refresh_snapshots: RefCell::new(refresh_snapshots),
			refresh_error: RefCell::new(None),
			refresh_queries: RefCell::new(Vec::new()),
			label_queries: RefCell::new(Vec::new()),
			comment_queries: RefCell::new(Vec::new()),
			comments: RefCell::new(Vec::new()),
			issue_comments: RefCell::new(HashMap::new()),
			state_updates: RefCell::new(Vec::new()),
			label_updates: RefCell::new(Vec::new()),
			label_additions: RefCell::new(Vec::new()),
			label_removals: RefCell::new(Vec::new()),
			next_created_issue_number: RefCell::new(0),
		}
	}

	pub(super) fn with_refresh_error(listed_issues: Vec<TrackerIssue>, message: &str) -> Self {
		let tracker = Self::with_refresh_snapshots_and_project(
			listed_issues.clone(),
			vec![listed_issues],
			true,
		);

		*tracker.refresh_error.borrow_mut() = Some(message.to_owned());

		tracker
	}

	pub(super) fn with_identifier_lookup_issues(mut self, issues: Vec<TrackerIssue>) -> Self {
		self.identifier_lookup_issues = Some(issues);

		self
	}

	pub(super) fn with_label_lookup_issues(
		mut self,
		label_name: &str,
		issues: Vec<TrackerIssue>,
	) -> Self {
		self.issues_by_label.insert(label_name.to_owned(), issues);

		self
	}

	pub(super) fn with_team_label_lookup_id(
		mut self,
		team_id: &str,
		label_name: &str,
		label_id: &str,
	) -> Self {
		self.team_label_ids_by_name
			.insert((team_id.to_owned(), label_name.to_owned()), label_id.to_owned());

		self
	}

	#[allow(dead_code)]
	pub(super) fn with_resolved_project_slug(self, _project_slug: &str) -> Self {
		self
	}

	#[allow(dead_code)]
	pub(super) fn with_required_list_project_slug(self, _project_slug: &str) -> Self {
		self
	}

	pub(super) fn with_project_lookup_error(self, _message: &str) -> Self {
		self
	}
}

impl IssueTracker for FakeTracker {
	fn list_issues_with_label(&self, label_name: &str) -> Result<Vec<TrackerIssue>> {
		self.label_queries.borrow_mut().push(label_name.to_owned());

		if let Some(issues) = self.issues_by_label.get(label_name) {
			return Ok(issues.clone());
		}

		Ok(self.listed_issues.iter().filter(|issue| issue.has_label(label_name)).cloned().collect())
	}

	fn find_team_label_id(&self, team_id: &str, label_name: &str) -> Result<Option<String>> {
		if let Some(label_id) =
			self.team_label_ids_by_name.get(&(team_id.to_owned(), label_name.to_owned()))
		{
			return Ok(Some(label_id.clone()));
		}

		Ok(self
			.listed_issues
			.iter()
			.find(|issue| issue.team.id == team_id)
			.and_then(|issue| issue.label_id_for_name(label_name).map(ToOwned::to_owned)))
	}

	fn get_issue_by_identifier(&self, issue_identifier: &str) -> Result<Option<TrackerIssue>> {
		self.identifier_queries.borrow_mut().push(issue_identifier.to_owned());

		let issues = self.identifier_lookup_issues.as_ref().unwrap_or(&self.listed_issues);

		Ok(issues
			.iter()
			.find(|issue| issue.identifier.eq_ignore_ascii_case(issue_identifier))
			.cloned())
	}

	fn refresh_issues(&self, issue_ids: &[String]) -> Result<Vec<TrackerIssue>> {
		self.refresh_queries.borrow_mut().push(issue_ids.to_vec());

		if let Some(message) = self.refresh_error.borrow_mut().take() {
			return Err(Report::msg(message));
		}

		let snapshot = {
			let mut refresh_snapshots = self.refresh_snapshots.borrow_mut();

			if refresh_snapshots.is_empty() {
				self.listed_issues.clone()
			} else {
				refresh_snapshots.remove(0)
			}
		};

		Ok(snapshot
			.iter()
			.filter(|issue| issue_ids.iter().any(|issue_id| issue_id == &issue.id))
			.cloned()
			.collect())
	}

	fn list_comments(&self, issue_id: &str) -> Result<Vec<TrackerComment>> {
		self.comment_queries.borrow_mut().push(issue_id.to_owned());

		Ok(self.issue_comments.borrow().get(issue_id).cloned().unwrap_or_default())
	}

	fn update_issue_state(&self, _issue_id: &str, _state_id: &str) -> Result<()> {
		self.state_updates.borrow_mut().push((_issue_id.to_owned(), _state_id.to_owned()));

		Ok(())
	}

	fn add_issue_labels(&self, _issue_id: &str, _label_ids: &[String]) -> Result<()> {
		self.label_additions.borrow_mut().push((_issue_id.to_owned(), _label_ids.to_vec()));

		Ok(())
	}

	fn remove_issue_labels(&self, _issue_id: &str, _label_ids: &[String]) -> Result<()> {
		self.label_removals.borrow_mut().push((_issue_id.to_owned(), _label_ids.to_vec()));

		Ok(())
	}

	fn create_comment(&self, _issue_id: &str, body: &str) -> Result<()> {
		self.comments.borrow_mut().push(body.to_owned());
		self.issue_comments.borrow_mut().entry(_issue_id.to_owned()).or_default().push(
			TrackerComment {
				body: body.to_owned(),
				created_at: String::from("2026-04-12T00:00:00Z"),
			},
		);

		Ok(())
	}

	fn create_issue(&self, request: &TrackerIssueCreate) -> Result<TrackerIssue> {
		let identifier = {
			let mut next_issue_number = self.next_created_issue_number.borrow_mut();

			*next_issue_number += 1;

			format!("PUB-G{}", *next_issue_number)
		};
		let state_name = request
			.state_id
			.as_deref()
			.and_then(|state_id| {
				self.listed_issues
					.iter()
					.flat_map(|issue| issue.team.states.iter())
					.find(|state| state.id == state_id)
					.map(|state| state.name.as_str())
			})
			.unwrap_or("Todo");
		let mut issue = tests::sample_issue_with_sort_fields(
			&format!("issue-{identifier}"),
			&identifier,
			state_name,
			&[],
			None,
			"2026-06-23T00:00:00Z",
		);

		issue.team.id.clone_from(&request.team_id);
		issue.title.clone_from(&request.title);
		issue.description.clone_from(&request.description);

		Ok(issue)
	}
}
