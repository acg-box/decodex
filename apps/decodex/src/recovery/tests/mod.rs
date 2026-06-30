use std::{cell::RefCell, fs, path::Path};

use tempfile::TempDir;

use crate::{
	config::ServiceConfig,
	pull_request::PullRequestLandingState,
	recovery::{
		GHOST_LANE_BLOCKED_CLASSIFICATION, GHOST_LANE_CLASSIFICATION, GHOST_LANE_CLEANUP_EVENT,
		GHOST_LANE_TERMINAL_STATUS, MCP_TEST_FIXTURE_GHOST_LANE_CLASSIFICATION,
		REVIEW_HANDOFF_ADOPT_EVENT, REVIEW_HANDOFF_BOUND_CLASSIFICATION,
		REVIEW_HANDOFF_OWNERSHIP_DRIFT_CLASSIFICATION, REVIEW_HANDOFF_REBIND_EVENT,
		REVIEW_HANDOFF_REBIND_REQUIRED_CLASSIFICATION, RUN_CONTROL_CHANNEL_STATUS_FAILED,
		STALE_ACTIVE_CLASSIFICATION, STALE_ACTIVE_RECOVERY_SCHEMA, STALE_ACTIVE_RELEASE_EVENT,
	},
	state::{
		self, ChildAgentActivitySummary, ConnectorBackoffInput, ProtocolActivityMarker,
		ProtocolActivitySummary, ReviewHandoffMarker, ReviewOrchestrationMarker,
		ReviewPolicyCheckpointInput, StateStore, WorktreeMapping,
	},
	tracker::{
		self, IssueTracker, TrackerComment, TrackerIssue, TrackerLabel, TrackerState, TrackerTeam,
		linear::LinearClient,
		records::{self, LinearExecutionEventIdentity, LinearExecutionEventRecord},
	},
	workflow::WorkflowDocument,
};

struct GhostLaneTestTracker {
	issues: Vec<TrackerIssue>,
	refresh_error: Option<String>,
	identifier_error: Option<String>,
	remove_error: Option<String>,
	comments: Vec<TrackerComment>,
	refresh_queries: RefCell<Vec<Vec<String>>>,
	label_removals: RefCell<Vec<(String, Vec<String>)>>,
	state_updates: RefCell<Vec<(String, String)>>,
}
impl GhostLaneTestTracker {
	fn missing() -> Self {
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

	fn with_issues(issues: Vec<TrackerIssue>) -> Self {
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

	fn with_comments(mut self, comments: Vec<TrackerComment>) -> Self {
		self.comments = comments;
		self
	}

	fn remove_error(mut self, message: &str) -> Self {
		self.remove_error = Some(message.to_owned());
		self
	}

	fn refresh_error(message: &str) -> Self {
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

	fn identifier_error(message: &str) -> Self {
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
	fn list_issues_with_label(
		&self,
		label_name: &str,
	) -> crate::prelude::Result<Vec<TrackerIssue>> {
		Ok(self.issues.iter().filter(|issue| issue.has_label(label_name)).cloned().collect())
	}

	fn find_team_label_id(
		&self,
		team_id: &str,
		label_name: &str,
	) -> crate::prelude::Result<Option<String>> {
		Ok(self
			.issues
			.iter()
			.find(|issue| issue.team.id == team_id)
			.and_then(|issue| issue.label_id_for_name(label_name).map(ToOwned::to_owned)))
	}

	fn get_issue_by_identifier(
		&self,
		issue_identifier: &str,
	) -> crate::prelude::Result<Option<TrackerIssue>> {
		if let Some(message) = &self.identifier_error {
			return Err(crate::prelude::eyre::eyre!(message.clone()));
		}

		Ok(self
			.issues
			.iter()
			.find(|issue| issue.identifier.eq_ignore_ascii_case(issue_identifier))
			.cloned())
	}

	fn refresh_issues(&self, issue_ids: &[String]) -> crate::prelude::Result<Vec<TrackerIssue>> {
		self.refresh_queries.borrow_mut().push(issue_ids.to_vec());

		if let Some(message) = &self.refresh_error {
			return Err(crate::prelude::eyre::eyre!(message.clone()));
		}

		Ok(self
			.issues
			.iter()
			.filter(|issue| issue_ids.iter().any(|issue_id| issue_id == &issue.id))
			.cloned()
			.collect())
	}

	fn list_comments(&self, _issue_id: &str) -> crate::prelude::Result<Vec<TrackerComment>> {
		Ok(self.comments.clone())
	}

	fn update_issue_state(&self, issue_id: &str, state_id: &str) -> crate::prelude::Result<()> {
		self.state_updates.borrow_mut().push((issue_id.to_owned(), state_id.to_owned()));
		Ok(())
	}

	fn add_issue_labels(
		&self,
		_issue_id: &str,
		_label_ids: &[String],
	) -> crate::prelude::Result<()> {
		Ok(())
	}

	fn remove_issue_labels(
		&self,
		issue_id: &str,
		label_ids: &[String],
	) -> crate::prelude::Result<()> {
		self.label_removals.borrow_mut().push((issue_id.to_owned(), label_ids.to_vec()));
		if let Some(message) = &self.remove_error {
			return Err(crate::prelude::eyre::eyre!(message.clone()));
		}

		Ok(())
	}

	fn create_comment(&self, _issue_id: &str, _body: &str) -> crate::prelude::Result<()> {
		Ok(())
	}
}

struct FinalNeedsAttentionTracker {
	issue: TrackerIssue,
	needs_attention_label: String,
	get_issue_calls: RefCell<usize>,
	label_removals: RefCell<Vec<(String, Vec<String>)>>,
}
impl FinalNeedsAttentionTracker {
	fn new(issue: TrackerIssue, needs_attention_label: String) -> Self {
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
	fn list_issues_with_label(
		&self,
		label_name: &str,
	) -> crate::prelude::Result<Vec<TrackerIssue>> {
		let issue = self.issue_for_call(*self.get_issue_calls.borrow());

		Ok(issue.has_label(label_name).then_some(issue).into_iter().collect())
	}

	fn find_team_label_id(
		&self,
		_team_id: &str,
		label_name: &str,
	) -> crate::prelude::Result<Option<String>> {
		Ok(Some(format!("label-{}", label_name.replace(':', "-"))))
	}

	fn get_issue_by_identifier(
		&self,
		issue_identifier: &str,
	) -> crate::prelude::Result<Option<TrackerIssue>> {
		let mut calls = self.get_issue_calls.borrow_mut();

		*calls += 1;
		let issue = self.issue_for_call(*calls);

		Ok((issue.identifier == issue_identifier).then_some(issue))
	}

	fn refresh_issues(&self, issue_ids: &[String]) -> crate::prelude::Result<Vec<TrackerIssue>> {
		let issue = self.issue_for_call(*self.get_issue_calls.borrow());

		Ok(issue_ids
			.iter()
			.any(|issue_id| issue_id == &issue.id)
			.then_some(issue)
			.into_iter()
			.collect())
	}

	fn list_comments(&self, _issue_id: &str) -> crate::prelude::Result<Vec<TrackerComment>> {
		Ok(Vec::new())
	}

	fn update_issue_state(&self, _issue_id: &str, _state_id: &str) -> crate::prelude::Result<()> {
		Ok(())
	}

	fn add_issue_labels(
		&self,
		_issue_id: &str,
		_label_ids: &[String],
	) -> crate::prelude::Result<()> {
		Ok(())
	}

	fn remove_issue_labels(
		&self,
		issue_id: &str,
		label_ids: &[String],
	) -> crate::prelude::Result<()> {
		self.label_removals.borrow_mut().push((issue_id.to_owned(), label_ids.to_vec()));

		Ok(())
	}

	fn create_comment(&self, _issue_id: &str, _body: &str) -> crate::prelude::Result<()> {
		Ok(())
	}
}

fn sample_worktree(branch_name: &str) -> WorktreeMapping {
	sample_worktree_at(branch_name, Path::new("/tmp/PUB-718"))
}

fn sample_worktree_at(branch_name: &str, worktree_path: &Path) -> WorktreeMapping {
	let store = StateStore::open_in_memory().expect("state store should open");
	let worktree_path = worktree_path.to_string_lossy();

	store
		.upsert_worktree("pubfi", "issue-id", branch_name, &worktree_path)
		.expect("worktree should persist");

	store
		.worktree_for_issue("issue-id")
		.expect("worktree should read")
		.expect("worktree should exist")
}

fn sample_landing_state(
	pr_url: &str,
	branch_name: &str,
	head_oid: &str,
) -> PullRequestLandingState {
	PullRequestLandingState {
		url: pr_url.to_owned(),
		state: String::from("OPEN"),
		is_draft: false,
		review_decision: Some(String::from("APPROVED")),
		base_ref_name: String::from("main"),
		pending_review_requests: 0,
		mergeable: String::from("MERGEABLE"),
		merge_state_status: String::from("CLEAN"),
		head_ref_name: branch_name.to_owned(),
		head_ref_oid: head_oid.to_owned(),
		status_check_rollup_state: Some(String::from("SUCCESS")),
		unresolved_review_threads: 0,
	}
}

fn sample_workflow() -> WorkflowDocument {
	WorkflowDocument::parse_markdown(
		r#"
+++
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
max_turns = 8
max_retry_backoff_ms = 300000
gate_profiles = {}
canonicalize_commands = []
verify_commands = []

[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = []
timeout_seconds = 60

[context]
read_first = []
+++

Test workflow.
"#,
	)
	.expect("sample workflow should parse")
}

fn sample_recovery_context(
	temp_dir: &TempDir,
	runtime_mutation_policy: super::RecoveryRuntimeMutationPolicy,
) -> super::RecoveryContext {
	let repo_root = temp_dir.path().join("repo");
	let config_path = temp_dir.path().join("project.toml");

	fs::create_dir_all(&repo_root).expect("repo root should exist");
	fs::write(
		&config_path,
		r#"
service_id = "pubfi"

[paths]
repo_root = "repo"

[tracker]
api_key_env_var = "HOME"

[github]
token_env_var = "HOME"
"#,
	)
	.expect("config should write");

	super::RecoveryContext {
		config: ServiceConfig::from_path(&config_path).expect("config should load"),
		workflow: sample_workflow(),
		state_store: StateStore::open_in_memory().expect("state store should open"),
		tracker: LinearClient::new(String::from("test-token")).expect("linear client should build"),
		runtime_mutation_policy,
	}
}

fn seed_mcp_test_fixture_ghost_lane(store: &StateStore, worktree_root: &Path) {
	let channel_path = worktree_root.join("missing-run-control.channel");
	let protocol_activity = ProtocolActivitySummary {
		turn_status: Some(String::from("completed")),
		waiting_reason: Some(String::from("turn_completed")),
		rate_limit_status: None,
		recent_events: Vec::new(),
	};

	store.record_run_attempt("run-12", "PUB-012", 1, "running").expect("run attempt should record");
	store.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress").expect("lease should record");
	store.update_run_thread("run-12", "thread-12").expect("thread should record");
	store.update_run_turn("run-12", "turn-12").expect("turn should record");
	store
		.publish_run_control_channel_for_active_attempt("run-12", 1, &channel_path, "local_file")
		.expect("control channel row should publish");
	store
		.append_event("run-12", 1, "turn/completed", r#"{"status":"completed"}"#)
		.expect("protocol event should record");
	store
		.record_run_activity_summary("run-12", 1, None, Some(&protocol_activity))
		.expect("protocol activity should record");

	append_mcp_test_control_private_events(store);
}

fn append_mcp_test_control_private_events(store: &StateStore) {
	for (event_type, payload) in [
		(
			"control_action",
			serde_json::json!({
				"schema": "decodex.run_control_action/v1",
				"source": "mcp-test",
				"action": "steer"
			}),
		),
		(
			"control_action",
			serde_json::json!({
				"schema": "decodex.run_control_action/v1",
				"source": "cli",
				"action": "interrupt",
				"requested": {
					"project_id": "pubfi",
					"issue_id": "PUB-012",
					"run_id": "run-12",
					"attempt_number": 1,
					"thread_id": "thread-12",
					"turn_id": "turn-12"
				}
			}),
		),
		(
			"lane_control/steer/requested",
			serde_json::json!({
				"source": "mcp-test",
				"method": "turn/steer"
			}),
		),
		(
			"lane_control/interrupt/requested",
			serde_json::json!({
				"source": "mcp-test",
				"method": "turn/interrupt"
			}),
		),
	] {
		store
			.append_private_execution_event("pubfi", "PUB-012", "run-12", 1, event_type, payload)
			.expect("mcp-test private evidence should record");
	}
}

fn append_mcp_test_fixture_ghost_lane_cleanup_audit(store: &StateStore) {
	store
		.append_private_execution_event(
			"pubfi",
			"PUB-012",
			"run-12",
			1,
			GHOST_LANE_CLEANUP_EVENT,
			serde_json::json!({
				"schema": "decodex.ghost_lane_recovery_private_event/1",
				"event": GHOST_LANE_CLEANUP_EVENT,
				"classification": MCP_TEST_FIXTURE_GHOST_LANE_CLASSIFICATION,
				"reason": "tracker_issue_missing_and_only_mcp_test_control_fixture_evidence",
				"issue_identifier": "PUBFI-012",
				"terminal_status": GHOST_LANE_TERMINAL_STATUS,
				"cleared_run_lease": true,
				"evidence": [
					"tracker_issue_missing",
					"worktree_mapping_path_missing",
					"worktree_missing",
					"control_channel_file_missing",
					"mcp_test_fixture_control_channel_row_present",
					"mcp_test_fixture_private_control_evidence_present",
					"review_lineage_missing"
				],
				"blockers": [],
				"next_action": "ordinary automation may continue after status readback confirms no current attention lane"
			}),
		)
		.expect("cleanup audit should record");
}

fn sample_issue(state_name: &str) -> TrackerIssue {
	let states = vec![
		TrackerState { id: String::from("state-todo"), name: String::from("Todo") },
		TrackerState { id: String::from("state-progress"), name: String::from("In Progress") },
		TrackerState { id: String::from("state-review"), name: String::from("In Review") },
		TrackerState { id: String::from("state-done"), name: String::from("Done") },
	];
	let state = states
		.iter()
		.find(|state| state.name == state_name)
		.expect("sample state should exist")
		.clone();

	TrackerIssue {
		id: String::from("issue-id"),
		identifier: String::from("PUB-718"),
		#[cfg(test)]
		project_slug: None,
		title: String::from("Sample issue"),
		author: None,
		description: String::new(),
		priority: None,
		created_at: String::from("2026-06-09T00:00:00Z"),
		updated_at: String::from("2026-06-09T00:00:00Z"),
		state,
		team: TrackerTeam {
			id: String::from("team-id"),
			name: String::from("XY"),
			states,
			labels: Vec::new(),
		},
		labels_complete: true,
		labels: Vec::new(),
		blockers: Vec::new(),
	}
}

fn sample_issue_with_labels(state_name: &str, labels: &[String]) -> TrackerIssue {
	let mut issue = sample_issue(state_name);

	for label in labels {
		let tracker_label =
			TrackerLabel { id: format!("label-{}", label.replace(':', "-")), name: label.clone() };

		issue.team.labels.push(tracker_label.clone());
		issue.labels.push(tracker_label);
	}

	issue
}

fn init_git_repo(path: &Path) {
	fs::create_dir_all(path).expect("git repo path should create");
	let status = crate::test_support::hermetic_git_command()
		.arg("-C")
		.arg(path)
		.arg("init")
		.status()
		.expect("git init should run");

	assert!(status.success(), "git init should succeed");
}

fn commit_test_file(path: &Path, file_name: &str, body: &str, message: &str) {
	fs::write(path.join(file_name), body).expect("test file should write");
	run_git(path, &["add", file_name]);
	run_git(
		path,
		&[
			"-c",
			"user.name=Decodex Test",
			"-c",
			"user.email=decodex-test@example.invalid",
			"-c",
			"commit.gpgsign=false",
			"commit",
			"-m",
			message,
		],
	);
}

fn init_clean_git_repo_with_remote_default(path: &Path, branch_name: &str) {
	init_git_repo(path);
	run_git(path, &["checkout", "-B", "main"]);
	commit_test_file(path, "README.md", "base\n", "base");
	run_git(path, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
	run_git(path, &["symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/main"]);
	run_git(path, &["checkout", "-B", branch_name]);
}

fn run_git(repo: &Path, args: &[&str]) -> String {
	let output = crate::test_support::hermetic_git_command()
		.arg("-C")
		.arg(repo)
		.args(args)
		.output()
		.expect("git command should run");

	assert!(
		output.status.success(),
		"git {:?} failed: {}",
		args,
		String::from_utf8_lossy(&output.stderr)
	);

	String::from_utf8(output.stdout).expect("git stdout should be utf8").trim().to_owned()
}

fn commit_file(repo: &Path, contents: &str) -> String {
	fs::write(repo.join("tracked.txt"), contents).expect("tracked file should write");

	run_git(repo, &["add", "tracked.txt"]);
	run_git(repo, &["commit", "-m", "test commit"]);

	run_git(repo, &["rev-parse", "HEAD"])
}

fn temp_git_worktree(branch_name: &str) -> (TempDir, String, String) {
	let temp_dir = TempDir::new().expect("temp git repo should exist");
	let repo = temp_dir.path();

	run_git(repo, &["init"]);
	run_git(repo, &["config", "user.email", "decodex@example.invalid"]);
	run_git(repo, &["config", "user.name", "Decodex Test"]);
	run_git(repo, &["checkout", "-b", branch_name]);

	let first_head = commit_file(repo, "first\n");
	let second_head = commit_file(repo, "second\n");

	(temp_dir, first_head, second_head)
}

fn temp_rebased_git_worktree(branch_name: &str) -> (TempDir, String, String) {
	let (temp_dir, first_head, _) = temp_git_worktree(branch_name);
	let repo = temp_dir.path();

	run_git(repo, &["checkout", "--orphan", "rebased"]);
	run_git(repo, &["rm", "-rf", "."]);

	let rebased_head = commit_file(repo, "rebased\n");

	run_git(repo, &["branch", "-D", branch_name]);
	run_git(repo, &["branch", "-m", branch_name]);

	(temp_dir, first_head, rebased_head)
}

mod context;
mod event_records;
mod ghost_lane;
mod git_worktree;
mod review_handoff;
mod stale_active;
