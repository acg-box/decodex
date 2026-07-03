use std::{cell::RefCell, collections::HashMap};

use crate::{
	archive_hygiene::plan,
	config::ServiceConfig,
	prelude::Result,
	tracker::{
		self, IssueTracker, TrackerComment, TrackerIssue, TrackerIssueBlocker, TrackerLabel,
		TrackerState, TrackerTeam,
	},
	workflow::WorkflowDocument,
};

const WORKFLOW: &str = r#"+++
version = 1

[tracker]
provider = "linear"
startable_states = ["Todo"]
terminal_states = ["Done", "Canceled", "Duplicate"]
in_progress_state = "In Progress"
success_state = "In Review"
completed_state = "Done"
failure_state = "Todo"
opt_out_label = "decodex:manual-only"
needs_attention_label = "decodex:needs-attention"

[agent]
transport = "stdio://"

[execution]
max_attempts = 3
max_turns = 3
max_retry_backoff_ms = 300000
gate_profiles = {}
canonicalize_commands = ["cargo make fmt"]
verify_commands = ["cargo make test"]

[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = []
timeout_seconds = 60

[context]
read_first = []
+++
"#;

#[derive(Default)]
struct FakeArchiveTracker {
	issues_by_label: HashMap<String, Vec<TrackerIssue>>,
	archived_issue_ids: RefCell<Vec<String>>,
}
impl FakeArchiveTracker {
	fn with_label(mut self, label: &str, issues: Vec<TrackerIssue>) -> Self {
		self.issues_by_label.insert(label.to_owned(), issues);

		self
	}

	fn archive_issue(&self, issue_id: &str) {
		self.archived_issue_ids.borrow_mut().push(issue_id.to_owned());
	}
}

impl IssueTracker for FakeArchiveTracker {
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
}

#[test]
fn archive_plan_includes_only_old_terminal_repo_labeled_issues() {
	let config = service_config();
	let workflow = workflow();
	let active = tracker::automation_active_label(config.service_id());
	let queued = tracker::automation_queue_label(config.service_id());
	let tracker = FakeArchiveTracker::default().with_label(
		"repo:decodex",
		vec![
			issue("issue-old", "XY-1", "Done", &["repo:decodex"], "2026-03-01T00:00:00Z"),
			issue(
				"issue-active",
				"XY-2",
				"Done",
				&["repo:decodex", &active],
				"2026-03-01T00:00:00Z",
			),
			issue(
				"issue-queued",
				"XY-3",
				"Canceled",
				&["repo:decodex", &queued],
				"2026-03-01T00:00:00Z",
			),
			issue(
				"issue-needs",
				"XY-4",
				"Duplicate",
				&["repo:decodex", "decodex:needs-attention"],
				"2026-03-01T00:00:00Z",
			),
			issue(
				"issue-manual",
				"XY-5",
				"Done",
				&["repo:decodex", "decodex:manual-only"],
				"2026-03-01T00:00:00Z",
			),
			issue("issue-todo", "XY-6", "Todo", &["repo:decodex"], "2026-03-01T00:00:00Z"),
			issue("issue-new", "XY-7", "Done", &["repo:decodex"], "2026-04-20T00:00:00Z"),
			issue("issue-equal", "XY-8", "Done", &["repo:decodex"], "2026-04-01T00:00:00Z"),
		],
	);
	let plan = plan::build_archive_plan(
		&tracker,
		&config,
		&workflow,
		&[String::from("repo:decodex")],
		"2026-04-01T00:00:00Z",
	)
	.expect("archive plan should build");

	assert_eq!(
		plan.candidates.iter().map(|candidate| candidate.identifier.as_str()).collect::<Vec<_>>(),
		vec!["XY-1"]
	);
	assert_eq!(plan.skipped.len(), 7);
	assert!(
		plan.skipped
			.iter()
			.any(|skipped| skipped.reason.contains("protected label `decodex:active:decodex`"))
	);
	assert!(
		plan.skipped
			.iter()
			.any(|skipped| skipped.reason.contains("protected label `decodex:queued:decodex`"))
	);
	assert!(
		plan.skipped
			.iter()
			.any(|skipped| skipped.reason.contains("protected label `decodex:needs-attention`"))
	);
	assert!(
		plan.skipped
			.iter()
			.any(|skipped| skipped.reason.contains("protected label `decodex:manual-only`"))
	);
}

#[test]
fn archive_execution_uses_archive_mutation_only_for_candidates() {
	let config = service_config();
	let workflow = workflow();
	let tracker = FakeArchiveTracker::default().with_label(
		"repo:decodex",
		vec![
			issue("issue-old", "XY-1", "Done", &["repo:decodex"], "2026-03-01T00:00:00Z"),
			issue("issue-new", "XY-2", "Done", &["repo:decodex"], "2026-04-20T00:00:00Z"),
		],
	);
	let plan = plan::build_archive_plan(
		&tracker,
		&config,
		&workflow,
		&[String::from("repo:decodex")],
		"2026-04-01T00:00:00Z",
	)
	.expect("archive plan should build");

	for candidate in &plan.candidates {
		tracker.archive_issue(&candidate.id);
	}

	assert_eq!(tracker.archived_issue_ids.borrow().as_slice(), ["issue-old"]);
}

fn service_config() -> ServiceConfig {
	ServiceConfig::parse_toml(
		r#"
service_id = "decodex"

[tracker]
api_key_env_var = "LINEAR_API_KEY_TEST"

[github]
token_env_var = "GITHUB_TOKEN_TEST"

[paths]
repo_root = "."
"#,
	)
	.expect("config should parse")
}

fn workflow() -> WorkflowDocument {
	WorkflowDocument::parse_markdown(WORKFLOW).expect("workflow should parse")
}

fn issue(
	id: &str,
	identifier: &str,
	state: &str,
	labels: &[&str],
	updated_at: &str,
) -> TrackerIssue {
	let team_labels = [
		"repo:decodex",
		"decodex:active:decodex",
		"decodex:queued:decodex",
		"decodex:needs-attention",
		"decodex:manual-only",
	];

	TrackerIssue {
		id: id.to_owned(),
		identifier: identifier.to_owned(),
		#[cfg(test)]
		project_slug: Some(String::from("decodex")),
		title: format!("Issue {identifier}"),
		author: None,
		description: String::new(),
		priority: None,
		created_at: String::from("2026-02-01T00:00:00Z"),
		updated_at: updated_at.to_owned(),
		state: TrackerState { id: format!("state-{state}"), name: state.to_owned() },
		team: TrackerTeam {
			id: String::from("team-1"),
			name: String::from("Decodex"),
			states: Vec::new(),
			labels: team_labels
				.iter()
				.enumerate()
				.map(|(index, label)| TrackerLabel {
					id: format!("team-label-{index}"),
					name: (*label).to_owned(),
				})
				.collect(),
		},
		labels_complete: true,
		labels: labels
			.iter()
			.enumerate()
			.map(|(index, label)| TrackerLabel {
				id: format!("issue-label-{index}"),
				name: (*label).to_owned(),
			})
			.collect(),
		blockers: Vec::<TrackerIssueBlocker>::new(),
	}
}
