use std::{
	collections::{BTreeMap, BTreeSet},
	env,
	path::{Path, PathBuf},
};

use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	config::ServiceConfig,
	prelude::{Result, eyre},
	runtime,
	state::StateStore,
	tracker::{self, IssueTracker, TrackerIssue, linear::LinearClient},
	workflow::WorkflowDocument,
};

pub(crate) struct ArchiveHygieneRequest {
	pub(crate) repo_labels: Vec<String>,
	pub(crate) older_than_days: u32,
	pub(crate) execute: bool,
}

#[derive(Debug, Eq, PartialEq)]
struct ArchivePlan {
	candidates: Vec<ArchiveCandidate>,
	skipped: Vec<ArchiveSkip>,
}

#[derive(Debug, Eq, PartialEq)]
struct ArchiveCandidate {
	id: String,
	identifier: String,
	title: String,
	state: String,
	updated_at: String,
	repo_labels: Vec<String>,
}

#[derive(Debug, Eq, PartialEq)]
struct ArchiveSkip {
	identifier: String,
	reason: String,
}

pub(crate) fn run(config_path: Option<&Path>, request: &ArchiveHygieneRequest) -> Result<()> {
	let state_store = runtime::open_runtime_store()?;
	let Some(config_path) = resolve_config_path(config_path, &state_store)? else {
		eyre::bail!(
			"No Decodex project config found. Pass --config <PROJECT_DIR> or register one with `decodex project add <PROJECT_DIR>`."
		);
	};
	let config = ServiceConfig::from_path(&config_path)?;
	let workflow = WorkflowDocument::from_path(config.workflow_path())?;
	let tracker = LinearClient::new(config.tracker().resolve_api_key()?)?;
	let repo_labels = normalize_repo_labels(&request.repo_labels)?;
	let updated_before = updated_before_timestamp(request.older_than_days)?;
	let plan = build_archive_plan(&tracker, &config, &workflow, &repo_labels, &updated_before)?;

	print_archive_plan(&plan, &repo_labels, &updated_before, request.execute);

	if request.execute {
		for candidate in &plan.candidates {
			tracker.archive_issue(&candidate.id)?;
		}

		println!("Archived {} Linear issue(s).", plan.candidates.len());
	}

	Ok(())
}

fn resolve_config_path(
	explicit_path: Option<&Path>,
	state_store: &StateStore,
) -> Result<Option<PathBuf>> {
	if let Some(path) = explicit_path {
		return Ok(Some(path.to_path_buf()));
	}

	runtime::registered_config_path_for_cwd(state_store, &env::current_dir()?)
}

fn normalize_repo_labels(repo_labels: &[String]) -> Result<Vec<String>> {
	let mut labels = BTreeSet::new();

	for label in repo_labels {
		if label.trim() != label || label.is_empty() {
			eyre::bail!(
				"`--repo-label` values must be non-empty labels without surrounding whitespace."
			);
		}
		if !label.starts_with("repo:") {
			eyre::bail!("`--repo-label` must name a repo label such as `repo:decodex`.");
		}

		labels.insert(label.clone());
	}

	if labels.is_empty() {
		eyre::bail!("At least one `--repo-label` is required.");
	}

	Ok(labels.into_iter().collect())
}

fn updated_before_timestamp(older_than_days: u32) -> Result<String> {
	if older_than_days == 0 {
		eyre::bail!("`--older-than-days` must be greater than zero.");
	}

	Ok((OffsetDateTime::now_utc() - Duration::days(i64::from(older_than_days))).format(&Rfc3339)?)
}

fn build_archive_plan<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	repo_labels: &[String],
	updated_before: &str,
) -> Result<ArchivePlan>
where
	T: IssueTracker + ?Sized,
{
	let issues = collect_repo_labeled_issues(tracker, repo_labels)?;
	let mut candidates = Vec::new();
	let mut skipped = Vec::new();

	for (issue, matched_repo_labels) in issues {
		match archive_skip_reason(tracker, project, workflow, &issue, updated_before)? {
			Some(reason) => skipped.push(ArchiveSkip { identifier: issue.identifier, reason }),
			None => candidates.push(ArchiveCandidate {
				id: issue.id,
				identifier: issue.identifier,
				title: issue.title,
				state: issue.state.name,
				updated_at: issue.updated_at,
				repo_labels: matched_repo_labels,
			}),
		}
	}

	candidates.sort_by(|left, right| left.identifier.cmp(&right.identifier));
	skipped.sort_by(|left, right| left.identifier.cmp(&right.identifier));

	Ok(ArchivePlan { candidates, skipped })
}

fn collect_repo_labeled_issues<T>(
	tracker: &T,
	repo_labels: &[String],
) -> Result<Vec<(TrackerIssue, Vec<String>)>>
where
	T: IssueTracker + ?Sized,
{
	let mut issues_by_id: BTreeMap<String, (TrackerIssue, BTreeSet<String>)> = BTreeMap::new();

	for repo_label in repo_labels {
		for issue in tracker.list_issues_with_label(repo_label)? {
			let entry = issues_by_id.entry(issue.id.clone()).or_insert((issue, BTreeSet::new()));

			entry.1.insert(repo_label.clone());
		}
	}

	Ok(issues_by_id
		.into_values()
		.map(|(issue, labels)| (issue, labels.into_iter().collect()))
		.collect())
}

fn archive_skip_reason<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	issue: &TrackerIssue,
	updated_before: &str,
) -> Result<Option<String>>
where
	T: IssueTracker + ?Sized,
{
	let tracker_policy = workflow.frontmatter().tracker();

	if !tracker_policy.terminal_states().iter().any(|state| state == &issue.state.name) {
		return Ok(Some(format!(
			"state `{}` is not a configured terminal state",
			issue.state.name
		)));
	}
	if issue_updated_at_is_not_older_than_cutoff(issue, updated_before)? {
		return Ok(Some(format!(
			"updated at `{}` is not older than cutoff `{updated_before}`",
			issue.updated_at
		)));
	}

	for label in protected_labels(project.service_id(), workflow) {
		if tracker::issue_has_label_with_server_confirmation(tracker, issue, &label)? {
			return Ok(Some(format!("protected label `{label}` is present")));
		}
	}

	Ok(None)
}

fn issue_updated_at_is_not_older_than_cutoff(
	issue: &TrackerIssue,
	updated_before: &str,
) -> Result<bool> {
	let issue_updated_at = OffsetDateTime::parse(&issue.updated_at, &Rfc3339).map_err(|error| {
		eyre::eyre!(
			"Failed to parse Linear updatedAt `{}` for issue `{}`: {error}",
			issue.updated_at,
			issue.identifier
		)
	})?;
	let cutoff = OffsetDateTime::parse(updated_before, &Rfc3339).map_err(|error| {
		eyre::eyre!("Failed to parse archive cutoff `{updated_before}`: {error}")
	})?;

	Ok(issue_updated_at >= cutoff)
}

fn protected_labels(service_id: &str, workflow: &WorkflowDocument) -> Vec<String> {
	let tracker_policy = workflow.frontmatter().tracker();

	vec![
		tracker::automation_active_label(service_id),
		tracker::automation_queue_label(service_id),
		tracker_policy.needs_attention_label().to_owned(),
		tracker_policy.opt_out_label().to_owned(),
	]
}

fn print_archive_plan(
	plan: &ArchivePlan,
	repo_labels: &[String],
	updated_before: &str,
	execute: bool,
) {
	let mode = if execute { "execute" } else { "dry run" };

	println!("Linear tracker archive hygiene ({mode})");
	println!("Repo labels: {}", repo_labels.join(", "));
	println!("Updated before: {updated_before}");
	println!("Archive candidates: {}", plan.candidates.len());

	for candidate in &plan.candidates {
		println!(
			"- {} [{}] updated={} labels={} title={}",
			candidate.identifier,
			candidate.state,
			candidate.updated_at,
			candidate.repo_labels.join(","),
			candidate.title
		);
	}

	if !plan.skipped.is_empty() {
		println!("Skipped: {}", plan.skipped.len());

		for skipped in &plan.skipped {
			println!("- {}: {}", skipped.identifier, skipped.reason);
		}
	}
}

#[cfg(test)]
mod tests {
	use std::{cell::RefCell, collections::HashMap};

	use crate::{
		archive_hygiene::{
			self, IssueTracker, Result, ServiceConfig, TrackerIssue, WorkflowDocument,
		},
		tracker::{self, TrackerIssueBlocker, TrackerLabel, TrackerState, TrackerTeam},
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
max_concurrent_agents = 3
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

		fn list_comments(&self, _issue_id: &str) -> Result<Vec<tracker::TrackerComment>> {
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
		let plan = archive_hygiene::build_archive_plan(
			&tracker,
			&config,
			&workflow,
			&[String::from("repo:decodex")],
			"2026-04-01T00:00:00Z",
		)
		.expect("archive plan should build");

		assert_eq!(
			plan.candidates
				.iter()
				.map(|candidate| candidate.identifier.as_str())
				.collect::<Vec<_>>(),
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
			plan.skipped.iter().any(|skipped| skipped
				.reason
				.contains("protected label `decodex:needs-attention`"))
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
		let plan = archive_hygiene::build_archive_plan(
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
}
