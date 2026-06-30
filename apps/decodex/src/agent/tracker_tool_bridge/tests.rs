use std::{
	cell::RefCell,
	collections::HashMap,
	env,
	ffi::OsString,
	fs,
	path::{Path, PathBuf},
	process,
};

use serde_json::Value;
use tempfile::TempDir;

use crate::{
	agent::tracker_tool_bridge::{
		DynamicToolCallResponse, DynamicToolContentItem, DynamicToolHandler,
		ISSUE_COMMENT_TOOL_NAME, ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME,
		ISSUE_LABEL_ADD_TOOL_NAME, ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
		ISSUE_REVIEW_CHECKPOINT_TOOL_NAME, ISSUE_REVIEW_HANDOFF_TOOL_NAME,
		ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME, ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
		ISSUE_TRANSITION_TOOL_NAME, LocalRepoDetails, LocalRepoInspector, PullRequestDetails,
		PullRequestInspector, ReviewExecutionMode, ReviewHandoffContext,
		ReviewHandoffWritebackFailed, ReviewPolicyStopReason, ReviewPolicyStopRequested,
		RunCompletionDisposition, TrackerToolBridge, TurnCompletionStatus,
	},
	config::ReviewLevel,
	prelude::eyre,
	state::{
		ReviewCheckpointArtifactLookup, ReviewHandoffMarker, ReviewOrchestrationMarker,
		ReviewPolicyCheckpoint, ReviewPolicyCheckpointInput, StateStore,
	},
	tracker::{
		IssueTracker, TrackerComment, TrackerIssue, TrackerLabel, TrackerState, TrackerTeam,
		privacy_classifier::{
			PublicProjectionPrivacyClassification, PublicProjectionPrivacyClassifier,
		},
		records,
	},
	workflow::WorkflowDocument,
};

// Tracker mutation policy for active execution turns.
include!("tests/mutation/dispatch.rs");
include!("tests/mutation/continuation.rs");
include!("tests/mutation/progress.rs");

// Review handoff, repair, closeout, and Decodex Review policy.
include!("tests/review/policy.rs");
include!("tests/review/handoff.rs");

const TEST_SERVICE_ID: &str = "pubfi";

struct FakeTracker {
	state_updates: RefCell<Vec<String>>,
	label_updates: RefCell<Vec<Vec<String>>>,
	label_additions: RefCell<Vec<Vec<String>>>,
	label_removals: RefCell<Vec<Vec<String>>>,
	comments: RefCell<Vec<String>>,
	issue_comments: RefCell<HashMap<String, Vec<TrackerComment>>>,
	refresh_snapshots: RefCell<Vec<Vec<TrackerIssue>>>,
	issues_by_label: RefCell<HashMap<String, Vec<TrackerIssue>>>,
	team_label_ids_by_name: RefCell<HashMap<(String, String), String>>,
	fail_state_update: RefCell<Option<String>>,
	fail_label_update: RefCell<Option<String>>,
	fail_comment: RefCell<Option<String>>,
}
impl FakeTracker {
	fn new() -> Self {
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

	fn with_refresh_snapshots(refresh_snapshots: Vec<Vec<TrackerIssue>>) -> Self {
		let tracker = Self::new();

		tracker.refresh_snapshots.replace(refresh_snapshots);

		tracker
	}

	fn with_state_update_error(message: &str) -> Self {
		let tracker = Self::new();

		tracker.fail_state_update.replace(Some(message.to_owned()));

		tracker
	}

	fn with_label_update_error(message: &str) -> Self {
		let tracker = Self::new();

		tracker.fail_label_update.replace(Some(message.to_owned()));

		tracker
	}

	fn with_comment_error(message: &str) -> Self {
		let tracker = Self::new();

		tracker.fail_comment.replace(Some(message.to_owned()));

		tracker
	}

	fn with_label_lookup_issues(self, label_name: &str, issues: Vec<TrackerIssue>) -> Self {
		self.issues_by_label.borrow_mut().insert(label_name.to_owned(), issues);

		self
	}

	fn with_team_label_lookup_id(self, team_id: &str, label_name: &str, label_id: &str) -> Self {
		self.team_label_ids_by_name
			.borrow_mut()
			.insert((team_id.to_owned(), label_name.to_owned()), label_id.to_owned());

		self
	}
}

impl IssueTracker for FakeTracker {
	fn list_issues_with_label(
		&self,
		label_name: &str,
	) -> crate::prelude::Result<Vec<TrackerIssue>> {
		Ok(self.issues_by_label.borrow().get(label_name).cloned().unwrap_or_default())
	}

	fn find_team_label_id(
		&self,
		team_id: &str,
		label_name: &str,
	) -> crate::prelude::Result<Option<String>> {
		Ok(self
			.team_label_ids_by_name
			.borrow()
			.get(&(team_id.to_owned(), label_name.to_owned()))
			.cloned())
	}

	fn get_issue_by_identifier(
		&self,
		_issue_identifier: &str,
	) -> crate::prelude::Result<Option<TrackerIssue>> {
		Ok(None)
	}

	fn refresh_issues(&self, _issue_ids: &[String]) -> crate::prelude::Result<Vec<TrackerIssue>> {
		if self.refresh_snapshots.borrow().is_empty() {
			return Ok(Vec::new());
		}

		Ok(self.refresh_snapshots.borrow_mut().remove(0))
	}

	fn list_comments(&self, issue_id: &str) -> crate::prelude::Result<Vec<TrackerComment>> {
		Ok(self.issue_comments.borrow().get(issue_id).cloned().unwrap_or_default())
	}

	fn update_issue_state(&self, _issue_id: &str, state_id: &str) -> crate::prelude::Result<()> {
		if let Some(message) = self.fail_state_update.borrow().as_ref() {
			return Err(eyre::eyre!(message.clone()));
		}

		self.state_updates.borrow_mut().push(state_id.to_owned());

		Ok(())
	}

	fn add_issue_labels(
		&self,
		_issue_id: &str,
		label_ids: &[String],
	) -> crate::prelude::Result<()> {
		if let Some(message) = self.fail_label_update.borrow().as_ref() {
			return Err(eyre::eyre!(message.clone()));
		}

		self.label_additions.borrow_mut().push(label_ids.to_vec());

		Ok(())
	}

	fn remove_issue_labels(
		&self,
		_issue_id: &str,
		label_ids: &[String],
	) -> crate::prelude::Result<()> {
		if let Some(message) = self.fail_label_update.borrow().as_ref() {
			return Err(eyre::eyre!(message.clone()));
		}

		self.label_removals.borrow_mut().push(label_ids.to_vec());

		Ok(())
	}

	fn create_comment(&self, _issue_id: &str, body: &str) -> crate::prelude::Result<()> {
		if let Some(message) = self.fail_comment.borrow().as_ref() {
			return Err(eyre::eyre!(message.clone()));
		}

		self.comments.borrow_mut().push(body.to_owned());
		self.issue_comments.borrow_mut().entry(_issue_id.to_owned()).or_default().push(
			TrackerComment {
				body: body.to_owned(),
				created_at: String::from("2026-04-12T00:00:00Z"),
			},
		);

		Ok(())
	}
}

struct FakePullRequestInspector {
	responses: RefCell<Vec<std::result::Result<PullRequestDetails, String>>>,
}
impl FakePullRequestInspector {
	fn new(responses: Vec<std::result::Result<PullRequestDetails, String>>) -> Self {
		Self { responses: RefCell::new(responses) }
	}
}

impl PullRequestInspector for FakePullRequestInspector {
	fn inspect_pull_request(
		&self,
		_cwd: &Path,
		_pr_url: &str,
		_github_token: &str,
		_gh_command_path: Option<&Path>,
	) -> std::result::Result<PullRequestDetails, String> {
		self.responses.borrow_mut().remove(0)
	}
}

struct GitHubTokenAssertingPullRequestInspector {
	expected_token: String,
	response: PullRequestDetails,
}
impl PullRequestInspector for GitHubTokenAssertingPullRequestInspector {
	fn inspect_pull_request(
		&self,
		_cwd: &Path,
		_pr_url: &str,
		github_token: &str,
		_gh_command_path: Option<&Path>,
	) -> std::result::Result<PullRequestDetails, String> {
		assert_eq!(github_token, self.expected_token.as_str());

		Ok(self.response.clone())
	}
}

struct FakeLocalRepoInspector {
	responses: RefCell<Vec<std::result::Result<LocalRepoDetails, String>>>,
}
impl FakeLocalRepoInspector {
	fn new(responses: Vec<std::result::Result<LocalRepoDetails, String>>) -> Self {
		Self { responses: RefCell::new(responses) }
	}
}

impl LocalRepoInspector for FakeLocalRepoInspector {
	fn inspect_local_repo(&self, _cwd: &Path) -> std::result::Result<LocalRepoDetails, String> {
		let mut responses = self.responses.borrow_mut();

		match responses.len() {
			0 => panic!("fake local repo inspector ran out of responses"),
			1 => responses[0].clone(),
			_ => responses.remove(0),
		}
	}
}

struct TestEnvVarGuard {
	key: String,
	previous: Option<OsString>,
}
impl TestEnvVarGuard {
	fn set(key: impl Into<String>, value: &str) -> Self {
		let key = key.into();
		let previous = env::var_os(&key);

		unsafe { env::set_var(&key, value) };

		Self { key, previous }
	}
}

impl Drop for TestEnvVarGuard {
	fn drop(&mut self) {
		match self.previous.take() {
			Some(previous) => unsafe { env::set_var(&self.key, previous) },
			None => unsafe { env::remove_var(&self.key) },
		}
	}
}

fn sample_issue() -> TrackerIssue {
	TrackerIssue {
		id: String::from("issue-1"),
		identifier: String::from("DEC-1"),
		#[cfg(test)]
		project_slug: Some(String::from("decodex")),
		title: String::from("Sample"),
		author: None,
		description: String::from("Body"),
		priority: Some(3),
		created_at: String::from("2026-03-13T04:16:17.133Z"),
		updated_at: String::from("2026-03-13T04:16:17.133Z"),
		state: TrackerState { id: String::from("state-todo"), name: String::from("Todo") },
		team: TrackerTeam {
			id: String::from("team-1"),
			name: String::from("Decodex"),
			states: vec![
				TrackerState { id: String::from("state-todo"), name: String::from("Todo") },
				TrackerState {
					id: String::from("state-progress"),
					name: String::from("In Progress"),
				},
				TrackerState { id: String::from("state-review"), name: String::from("In Review") },
			],
			labels: vec![
				TrackerLabel {
					id: String::from("label-queued"),
					name: crate::tracker::automation_queue_label(TEST_SERVICE_ID),
				},
				TrackerLabel {
					id: String::from("label-active"),
					name: crate::tracker::automation_active_label(TEST_SERVICE_ID),
				},
				TrackerLabel {
					id: String::from("label-manual"),
					name: String::from("decodex:manual-only"),
				},
				TrackerLabel {
					id: String::from("label-needs"),
					name: String::from("decodex:needs-attention"),
				},
			],
		},
		labels_complete: true,
		labels: Vec::new(),
		blockers: Vec::new(),
	}
}

fn sample_in_progress_issue() -> TrackerIssue {
	let mut issue = sample_issue();

	issue.state =
		TrackerState { id: String::from("state-progress"), name: String::from("In Progress") };

	issue
}

fn sample_review_issue() -> TrackerIssue {
	let mut issue = sample_issue();

	issue.state =
		TrackerState { id: String::from("state-review"), name: String::from("In Review") };

	issue
}

fn tracker_with_current_issue_snapshot(issue: &TrackerIssue) -> FakeTracker {
	FakeTracker::with_refresh_snapshots(vec![vec![issue.clone()]])
}

fn sample_workflow() -> WorkflowDocument {
	sample_workflow_with_tracker_states(&["Todo"], "In Progress", "In Review", "Todo")
}

fn sample_workflow_with_startable_states(startable_states: &[&str]) -> WorkflowDocument {
	sample_workflow_with_tracker_states(startable_states, "In Progress", "In Review", "Todo")
}

fn sample_workflow_with_tracker_states(
	startable_states: &[&str],
	in_progress_state: &str,
	success_state: &str,
	failure_state: &str,
) -> WorkflowDocument {
	let startable_states =
		startable_states.iter().map(|state| format!("\"{state}\"")).collect::<Vec<_>>().join(", ");

	WorkflowDocument::parse_markdown(&format!(
		r#"
+++
version = 1

[tracker]
provider = "linear"
startable_states = [{startable_states}]
terminal_states = ["Done", "Canceled", "Duplicate"]
in_progress_state = "{in_progress_state}"
success_state = "{success_state}"
completed_state = "Done"
failure_state = "{failure_state}"
opt_out_label = "decodex:manual-only"
needs_attention_label = "decodex:needs-attention"

[agent]
transport = "stdio://"

[execution]
max_attempts = 3
max_turns = 1
max_retry_backoff_ms = 300000
gate_profiles = {{}}
canonicalize_commands = []
verify_commands = []

[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = []
timeout_seconds = 60

[context]
read_first = []
+++

Use the tracker tools.
"#,
	))
	.expect("workflow should parse")
}

fn sample_review_context() -> ReviewHandoffContext {
	ReviewHandoffContext {
		attempt_number: 2,
		branch_name: String::from("x/decodex-pub-618"),
		run_id: String::from("pub-618-attempt-2-123"),
		service_id: String::from(TEST_SERVICE_ID),
		worktree_path: String::from(".worktrees/PUB-618"),
		cwd: PathBuf::from("/tmp/PUB-618"),
		github_token_env_var: Some(String::from("HOME")),
		github_command_path: None,
		review_level: ReviewLevel::Standard,
		mode: ReviewExecutionMode::Handoff,
		recorded_pr_url: None,
	}
}

fn manual_attention_comment_args() -> Value {
	serde_json::json!({
		"kind": "manual_attention",
		"error_class": "operator_decision_required",
		"next_action": "resolve the blocker manually, clear the needs-attention label, then restart automation if desired",
		"blockers": ["operator decision is required before automation can continue"],
		"evidence": ["agent selected the manual-attention path for this run"],
		"failed_command": "cargo make test",
		"raw_error": "repo gate failed with public test output"
	})
}

fn sample_review_context_in(cwd: &Path) -> ReviewHandoffContext {
	ReviewHandoffContext {
		attempt_number: 2,
		branch_name: String::from("x/decodex-pub-618"),
		run_id: String::from("pub-618-attempt-2-123"),
		service_id: String::from(TEST_SERVICE_ID),
		worktree_path: String::from(".worktrees/PUB-618"),
		cwd: cwd.to_path_buf(),
		github_token_env_var: Some(String::from("HOME")),
		github_command_path: None,
		review_level: ReviewLevel::Standard,
		mode: ReviewExecutionMode::Handoff,
		recorded_pr_url: None,
	}
}

fn sample_review_repair_context_in(cwd: &Path, pr_url: &str) -> ReviewHandoffContext {
	ReviewHandoffContext {
		attempt_number: 3,
		branch_name: String::from("x/decodex-pub-618"),
		run_id: String::from("pub-618-attempt-3-123"),
		service_id: String::from(TEST_SERVICE_ID),
		worktree_path: String::from(".worktrees/PUB-618"),
		cwd: cwd.to_path_buf(),
		github_token_env_var: Some(String::from("HOME")),
		github_command_path: None,
		review_level: ReviewLevel::Standard,
		mode: ReviewExecutionMode::Repair,
		recorded_pr_url: Some(pr_url.to_owned()),
	}
}

fn sample_closeout_context_in(cwd: &Path, pr_url: &str) -> ReviewHandoffContext {
	ReviewHandoffContext {
		attempt_number: 4,
		branch_name: String::from("x/decodex-pub-618"),
		run_id: String::from("pub-618-attempt-4-123"),
		service_id: String::from(TEST_SERVICE_ID),
		worktree_path: String::from(".worktrees/PUB-618"),
		cwd: cwd.to_path_buf(),
		github_token_env_var: Some(String::from("HOME")),
		github_command_path: None,
		review_level: ReviewLevel::Standard,
		mode: ReviewExecutionMode::Closeout,
		recorded_pr_url: Some(pr_url.to_owned()),
	}
}

fn sample_pull_request() -> PullRequestDetails {
	PullRequestDetails {
		head_ref_name: String::from("x/decodex-pub-618"),
		head_ref_oid: String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
		head_repository_name: String::from("decodex"),
		head_repository_owner: String::from("hack-ink"),
		is_draft: false,
		state: String::from("OPEN"),
		base_ref_name: String::from("main"),
		url: String::from("https://github.com/hack-ink/decodex/pull/48"),
	}
}

#[test]
fn review_blocking_status_keeps_source_changes_and_ignores_runtime_markers() {
	assert_eq!(
		super::review_blocking_status_lines(
			" M apps/decodex/src/agent/tracker_tool_bridge.rs\n\
			 ?? apps/decodex/src/agent/new_file.rs\n\
			 ?? .decodex-run-activity\n\
			 ?? .decodex-run-control/run-1.channel\n"
		),
		vec![
			String::from("M apps/decodex/src/agent/tracker_tool_bridge.rs"),
			String::from("?? apps/decodex/src/agent/new_file.rs"),
		]
	);
}

fn sample_local_repo() -> LocalRepoDetails {
	LocalRepoDetails {
		default_branch: String::from("main"),
		head_oid: String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
		head_tree_oid: String::from("f8a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
		repository_name: String::from("decodex"),
		repository_owner: String::from("hack-ink"),
		review_blocking_changes: Vec::new(),
	}
}

fn write_review_policy_checkpoint(
	bridge: &TrackerToolBridge<'_>,
	issue: &TrackerIssue,
	review_context: &ReviewHandoffContext,
	phase: &str,
	status: &str,
	head_sha: &str,
	nonclean_rounds: i64,
) {
	bridge_state_store(bridge)
		.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
			project_id: &review_context.service_id,
			issue_id: &issue.id,
			run_id: &review_context.run_id,
			attempt_number: review_context.attempt_number,
			phase,
			review_level: review_context.review_level.as_str(),
			status,
			head_sha,
			nonclean_rounds,
			details_json: "{}",
		})
		.expect("review policy state should write");
}

fn write_clean_review_checkpoint(
	bridge: &TrackerToolBridge<'_>,
	issue: &TrackerIssue,
	review_context: &ReviewHandoffContext,
) {
	let phase = match review_context.mode {
		ReviewExecutionMode::Handoff => "handoff",
		ReviewExecutionMode::Repair => "repair",
		ReviewExecutionMode::Closeout => {
			panic!("closeout does not support review checkpoints")
		},
	};

	write_review_policy_checkpoint(
		bridge,
		issue,
		review_context,
		phase,
		"clean",
		&sample_local_repo().head_oid,
		0,
	);
}

fn bridge_state_store<'a>(bridge: &TrackerToolBridge<'a>) -> &'a StateStore {
	bridge.state_store.expect("test bridge should have a runtime state store")
}

fn persisted_review_policy_checkpoint(
	bridge: &TrackerToolBridge<'_>,
	issue: &TrackerIssue,
	review_context: &ReviewHandoffContext,
) -> ReviewPolicyCheckpoint {
	let phase = match review_context.mode {
		ReviewExecutionMode::Handoff => "handoff",
		ReviewExecutionMode::Repair => "repair",
		ReviewExecutionMode::Closeout => {
			panic!("closeout does not support review checkpoints")
		},
	};

	bridge_state_store(bridge)
		.review_policy_checkpoint(
			&review_context.service_id,
			&issue.id,
			&review_context.run_id,
			review_context.attempt_number,
			phase,
		)
		.expect("review policy checkpoint should read")
		.expect("review policy checkpoint should exist")
}

fn assert_review_policy_checkpoint_cleared(
	bridge: &TrackerToolBridge<'_>,
	issue: &TrackerIssue,
	review_context: &ReviewHandoffContext,
) {
	let phase = match review_context.mode {
		ReviewExecutionMode::Handoff => "handoff",
		ReviewExecutionMode::Repair => "repair",
		ReviewExecutionMode::Closeout => {
			panic!("closeout does not support review checkpoints")
		},
	};

	assert!(
		bridge_state_store(bridge)
			.review_policy_checkpoint(
				&review_context.service_id,
				&issue.id,
				&review_context.run_id,
				review_context.attempt_number,
				phase,
			)
			.expect("review policy checkpoint should read")
			.is_none(),
		"review policy checkpoint should be cleared after completion"
	);
}

fn persisted_review_handoff_marker(
	bridge: &TrackerToolBridge<'_>,
	issue: &TrackerIssue,
	review_context: &ReviewHandoffContext,
) -> ReviewHandoffMarker {
	bridge_state_store(bridge)
		.review_handoff_marker(&review_context.service_id, &issue.id, &review_context.branch_name)
		.expect("review handoff marker should read")
		.expect("review handoff marker should exist")
}

fn persisted_review_orchestration_marker(
	bridge: &TrackerToolBridge<'_>,
	issue: &TrackerIssue,
	review_context: &ReviewHandoffContext,
	review_handoff: &ReviewHandoffMarker,
) -> ReviewOrchestrationMarker {
	bridge_state_store(bridge)
		.review_orchestration_marker(&review_context.service_id, &issue.id, review_handoff)
		.expect("review orchestration marker should read")
		.expect("review orchestration marker should exist")
}
