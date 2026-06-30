#[cfg(unix)] use std::os::fd::IntoRawFd;
use std::{
	cell::RefCell,
	collections::{BTreeSet, HashMap},
	env,
	ffi::{OsStr, OsString},
	fs,
	io::{Read as _, Write as _},
	iter,
	net::{Shutdown, TcpListener, TcpStream},
	path::{Path, PathBuf},
	process::{self, Command},
	sync::{Arc, Mutex},
	thread,
	time::{Duration, Instant},
};

use color_eyre::{Report, eyre};
use rusqlite::Connection;
use tempfile::TempDir;
use time::OffsetDateTime;

use crate::{
	orchestrator::RepoGatePhaseGoalController,
	tracker::{TrackerIssueCreate, records},
};
#[rustfmt::skip]
	use crate::agent::{
		RUN_LEASE_IDLE_TIMEOUT, MODEL_EXECUTION_IDLE_TIMEOUT,
		AppServerCapabilityPreflightFailure,
		AppServerDynamicToolFailure, AppServerHomePreflightFailure, AppServerPhaseGoalFailure,
		AppServerTransportFailure, AppServerTurnFailure,
		DynamicToolHandler, PhaseGoalController, PhaseGoalKind, PhaseGoalSpec,
		PhaseGoalTransition, ReviewPolicyStopReason, ReviewPolicyStopRequested,
		TrackerToolBridge, TurnContinuationGuard,
	};
#[rustfmt::skip]
use crate::config::{ReviewLevel, ServiceConfig};
#[rustfmt::skip]
use crate::github;
use crate::loop_contract::DecisionContract;
#[rustfmt::skip]
use crate::orchestrator::{self, CurrentChildRunContext, RunLeaseDisposition, RunLeaseReconciliation, ActiveWorkflowOverride, AgentEvidenceSource, AuthorityBoundaryChangedSurface, AuthorityBoundaryCheckInput, AuthorityBoundaryDisposition, AuthorityBoundaryPolicyDecision, AuthorityBoundarySurface, AuthorityDecisionRequestInput, ChildExitRetryContext, ChildRunRef, ControlPlaneProjectTick, CONTINUATION_PENDING_RUN_STATUS, DaemonRunChild, DaemonTickRuntimeContext, DashboardEventHub, EvidenceRequest, GhPullRequestReviewStateInspector, ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME, ISSUE_LABEL_ADD_TOOL_NAME, ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME, ISSUE_REVIEW_CHECKPOINT_TOOL_NAME, ISSUE_REVIEW_HANDOFF_TOOL_NAME, ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME, ISSUE_TERMINAL_FINALIZE_TOOL_NAME, ISSUE_TRANSITION_TOOL_NAME, IssueDispatchMode, IssueRunPlan, IssueTurnContinuationGuard, ManualAttentionRequested, OPERATOR_DASHBOARD_ALIAS_ENDPOINT_PATH, OPERATOR_DASHBOARD_ENDPOINT_PATH, PHASE_ACCEPTANCE_CHECK_EVENT_TYPE, PHASE_GOAL_RECOVERY_BLOCKED_EVENT_TYPE, PHASE_GOAL_RECOVERY_EVENT_TYPE, OperatorCodexAccountControlStatus, OperatorExecutionProgramNodeStatus, OperatorExecutionProgramStatus, OperatorGitHubCliAuthority, OperatorProjectStatus, OperatorStatusSnapshot, PostReviewLaneClassification, PostReviewLaneDecision, PostReviewLaneSnapshot, PreferredRunIdentity, PrepareIssueRunContext, PublishedOperatorSnapshot, PullRequestCommitConnection, PullRequestCommitNode, PullRequestCommitPayload, PullRequestIssueCommentConnection, PullRequestIssueCommentState, PullRequestIssueCommentsNode, PullRequestPageInfo, PullRequestReactionGroup, PullRequestReactionUsersConnection, PullRequestActor, PullRequestRepository, PullRequestRepositoryOwner, PullRequestReviewConnection, PullRequestIssueCommentNode, PullRequestReviewNode, PullRequestReviewRequestConnection, PullRequestReviewState, PullRequestReviewStateInspector, PullRequestReviewStateNode, PullRequestReviewStateRepository, PullRequestReviewSummaryState, PullRequestReviewThreadConnection, PullRequestReviewThreadNode, PullRequestStatusCheckRollup, RecoveredRuntimeState, RetainedPartialProgress, RetainedReviewRunIdentity, RetryComment, RetryDispatchDecision, RetryEntry, RetryKind, RetryQueue, RunCompletionDisposition, RunSummary, RepoGateFailure, TERMINAL_GUARD_MARKER_FILE, TERMINAL_GUARDED_RUN_STATUS, TRACKER_RATE_LIMIT_WARNING, TRACKER_TRANSIENT_TIMEOUT_WARNING, TargetIssueRunContext, EXTERNAL_REVIEW_ACTOR_LOGIN, EXTERNAL_REVIEW_PASS_PHRASE, EXTERNAL_REVIEW_REQUEST_BODY};
#[rustfmt::skip]
use crate::prelude::Result;
#[rustfmt::skip]
use crate::state::{
	self, ChildAgentActivitySummary, CodexAccountActivitySummary, CodexAccountMarker,
	EffectiveRuntimeMarker, ProjectRegistration, ProtocolActivityMarker, ProtocolActivitySummary,
	RUN_ACTIVITY_MARKER_FILE, RUN_OPERATION_AGENT_RUN, RUN_OPERATION_GIT_CREDENTIALS,
	RUN_OPERATION_RECONCILIATION, RUN_OPERATION_REPO_GATE, ReviewPolicyCheckpointInput,
	StateStore, WorktreeMapping,
};
use crate::test_support::TestEnvVarGuard;
#[rustfmt::skip]
use crate::tracker::{self, IssueTracker, TrackerComment, TrackerIssue, TrackerIssueBlocker, TrackerLabel, TrackerState, TrackerTeam, records::{LinearExecutionEventIdentity}};
#[rustfmt::skip]
use crate::workflow::WorkflowDocument;
#[rustfmt::skip]
use crate::worktree::{WorktreeManager, WorktreeSpec};
use crate::orchestrator::{ReviewHandoffMarker, ReviewOrchestrationMarker};

// Workflow reload, intake eligibility, prompting, and candidate selection.
include!("tests/intake/workflow_reload.rs");

include!("tests/intake/eligibility.rs");

include!("tests/intake/run_and_prompting.rs");

include!("tests/intake/prepare_issue_run.rs");

include!("tests/intake/candidate_selection.rs");

// Retry scheduling, runtime failure classes, and recovery cleanup.
include!("tests/retry/scheduling.rs");

include!("tests/retry/selection.rs");

include!("tests/runtime/repo_gate.rs");

include!("tests/runtime/failure.rs");

include!("tests/runtime/loop_scenarios.rs");

include!("tests/runtime/program_reconciler.rs");

include!("tests/runtime/program_intake_dogfood.rs");

include!("tests/runtime/thread_archive.rs");

include!("tests/recovery/reconciliation.rs");

include!("tests/recovery/terminal_support.rs");

include!("tests/recovery/closeout/dispatch.rs");

include!("tests/recovery/closeout/identity.rs");

include!("tests/recovery/closeout/cleanup.rs");

include!("tests/recovery/terminal_failures.rs");

include!("tests/recovery/runtime_reentry.rs");

// Operator status plus retained post-review review/landing behavior.
include!("tests/operator/status_support.rs");

include!("tests/operator/status/control_plane.rs");

include!("tests/operator/status/running_lanes.rs");

include!("tests/operator/status/history.rs");

include!("tests/operator/status/text.rs");

include!("tests/operator/status/publishing.rs");

include!("tests/operator/status/queue.rs");

include!("tests/operator/status/agent_evidence.rs");

include!("tests/operator/status/http.rs");

include!("tests/operator/status/dashboard.rs");

include!("tests/review_landing/status_support.rs");

include!("tests/review_landing/status_rows.rs");

include!("tests/review_landing/orchestration.rs");

include!("tests/review_landing/status_markers.rs");

include!("tests/review_landing/classification_review.rs");

include!("tests/review_landing/classification_checks.rs");

include!("tests/review_landing/review_state.rs");

const TEST_EXTERNAL_REVIEW_REQUEST_COMMENT_ID: i64 = 991;
const TEST_EXTERNAL_REVIEW_REQUEST_CREATED_AT: i64 = 1_763_600_000;
const TEST_EXTERNAL_REVIEW_AUTO_MERGE_ENABLED_AT: i64 = 1_763_600_120;
const TEST_NON_EXTERNAL_REVIEW_ACTOR_LOGIN: &str = "someone-else";
const TEST_SERVICE_ID: &str = "pubfi";
const TEST_PROJECT_CONFIG_FILE: &str = "project.toml";

struct FakeTracker {
	listed_issues: Vec<TrackerIssue>,
	identifier_lookup_issues: Option<Vec<TrackerIssue>>,
	issues_by_label: HashMap<String, Vec<TrackerIssue>>,
	team_label_ids_by_name: HashMap<(String, String), String>,
	identifier_queries: RefCell<Vec<String>>,
	refresh_snapshots: RefCell<Vec<Vec<TrackerIssue>>>,
	refresh_error: RefCell<Option<String>>,
	refresh_queries: RefCell<Vec<Vec<String>>>,
	label_queries: RefCell<Vec<String>>,
	comment_queries: RefCell<Vec<String>>,
	comments: RefCell<Vec<String>>,
	issue_comments: RefCell<HashMap<String, Vec<TrackerComment>>>,
	state_updates: RefCell<Vec<(String, String)>>,
	label_updates: RefCell<Vec<(String, Vec<String>)>>,
	label_additions: RefCell<Vec<(String, Vec<String>)>>,
	label_removals: RefCell<Vec<(String, Vec<String>)>>,
	next_created_issue_number: RefCell<usize>,
}
impl FakeTracker {
	fn new(issues: Vec<TrackerIssue>) -> Self {
		Self::with_refresh_snapshots_and_project(issues.clone(), vec![issues], true)
	}

	fn with_refresh_snapshots(
		listed_issues: Vec<TrackerIssue>,
		refresh_snapshots: Vec<Vec<TrackerIssue>>,
	) -> Self {
		Self::with_refresh_snapshots_and_project(listed_issues, refresh_snapshots, true)
	}

	fn with_refresh_snapshots_and_project(
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

	fn with_refresh_error(listed_issues: Vec<TrackerIssue>, message: &str) -> Self {
		let tracker = Self::with_refresh_snapshots_and_project(
			listed_issues.clone(),
			vec![listed_issues],
			true,
		);

		*tracker.refresh_error.borrow_mut() = Some(message.to_owned());

		tracker
	}

	fn with_identifier_lookup_issues(mut self, issues: Vec<TrackerIssue>) -> Self {
		self.identifier_lookup_issues = Some(issues);

		self
	}

	fn with_label_lookup_issues(mut self, label_name: &str, issues: Vec<TrackerIssue>) -> Self {
		self.issues_by_label.insert(label_name.to_owned(), issues);

		self
	}

	fn with_team_label_lookup_id(
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
	fn with_resolved_project_slug(self, _project_slug: &str) -> Self {
		self
	}

	#[allow(dead_code)]
	fn with_required_list_project_slug(self, _project_slug: &str) -> Self {
		self
	}

	fn with_project_lookup_error(self, _message: &str) -> Self {
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
		let mut issue = sample_issue_with_sort_fields(
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

struct FakePullRequestReviewStateInspector {
	responses: RefCell<Vec<Result<PullRequestReviewState>>>,
}
impl FakePullRequestReviewStateInspector {
	fn new(responses: Vec<Result<PullRequestReviewState>>) -> Self {
		Self { responses: RefCell::new(responses) }
	}
}

impl PullRequestReviewStateInspector for FakePullRequestReviewStateInspector {
	fn inspect_review_state(&self, _cwd: &Path, _pr_url: &str) -> Result<PullRequestReviewState> {
		self.responses.borrow_mut().remove(0)
	}
}

fn rewrite_run_activity_marker_host_boot_id(worktree_path: &Path, host_boot_id: &str) {
	let marker_path = worktree_path.join(RUN_ACTIVITY_MARKER_FILE);
	let marker_body = fs::read_to_string(&marker_path).expect("marker body should load");
	let mut host_boot_id_written = false;
	let mut rewritten = marker_body
		.lines()
		.map(|line| {
			if line.starts_with("host_boot_id=") {
				host_boot_id_written = true;

				format!("host_boot_id={host_boot_id}")
			} else {
				line.to_owned()
			}
		})
		.collect::<Vec<_>>();

	if !host_boot_id_written {
		rewritten.push(format!("host_boot_id={host_boot_id}"));
	}

	fs::write(&marker_path, rewritten.join("\n") + "\n").expect("marker body should rewrite");
}

fn rewrite_run_activity_marker_process_start_identity(
	worktree_path: &Path,
	process_start_identity: &str,
) {
	let marker_path = worktree_path.join(RUN_ACTIVITY_MARKER_FILE);
	let marker_body = fs::read_to_string(&marker_path).expect("marker body should load");
	let mut process_start_identity_written = false;
	let mut rewritten = marker_body
		.lines()
		.map(|line| {
			if line.starts_with("process_start_identity=") {
				process_start_identity_written = true;

				format!("process_start_identity={process_start_identity}")
			} else {
				line.to_owned()
			}
		})
		.collect::<Vec<_>>();

	if !process_start_identity_written {
		rewritten.push(format!("process_start_identity={process_start_identity}"));
	}

	fs::write(&marker_path, rewritten.join("\n") + "\n").expect("marker body should rewrite");
}

fn install_fake_post_issue_comment_gh_response(
	temp_dir: &TempDir,
	comment_id: i64,
	created_at: &str,
) -> TestEnvVarGuard {
	let fake_gh_dir = temp_dir.path().join("fake-bin");
	let fake_gh_path = fake_gh_dir.join("gh");
	let fake_gh_response = serde_json::json!({
		"id": comment_id,
		"created_at": created_at,
	})
	.to_string();

	fs::create_dir_all(&fake_gh_dir).expect("fake gh directory should exist");
	fs::write(&fake_gh_path, format!("#!/bin/sh\nprintf '%s' '{fake_gh_response}'\n"))
		.expect("fake gh script should write");

	let mut permissions =
		fs::metadata(&fake_gh_path).expect("fake gh metadata should read").permissions();

	#[cfg(unix)]
	PermissionsExt::set_mode(&mut permissions, 0o755);
	fs::set_permissions(&fake_gh_path, permissions)
		.expect("fake gh script should become executable");

	let path_env = env::var("PATH").unwrap_or_default();

	TestEnvVarGuard::set("PATH", &format!("{}:{path_env}", fake_gh_dir.display()))
}

fn install_fake_admin_merge_gh_response(temp_dir: &TempDir) -> (PathBuf, PathBuf) {
	install_fake_admin_merge_gh_response_with_merge_exit_code(temp_dir, "deadbeef", 0)
}

fn install_fake_admin_merge_gh_response_with_merge_exit_code(
	temp_dir: &TempDir,
	pr_head_oid: &str,
	merge_exit_code: i32,
) -> (PathBuf, PathBuf) {
	let fake_gh_dir = temp_dir.path().join("fake-bin");
	let fake_gh_path = fake_gh_dir.join("gh");
	let invocation_log_path = temp_dir.path().join("gh-invocation.log");

	fs::create_dir_all(&fake_gh_dir).expect("fake gh directory should exist");
	fs::write(
		&fake_gh_path,
		format!(
			"#!/bin/sh\n\
printf '%s\\n' \"$@\" >> '{}'\n\
if [ \"$1\" = \"pr\" ] && [ \"$2\" = \"merge\" ]; then\n\
  exit {}\n\
fi\n\
if [ \"$1\" = \"pr\" ] && [ \"$2\" = \"view\" ]; then\n\
  printf '%s' '{}'\n\
  exit 0\n\
fi\n\
echo \"unexpected gh invocation: $*\" >&2\n\
exit 1\n",
			invocation_log_path.display(),
			merge_exit_code,
			serde_json::json!({
				"state": "MERGED",
				"headRefOid": pr_head_oid,
				"mergeCommit": { "oid": "cafebabe" },
			}),
		),
	)
	.expect("fake gh script should write");

	let mut permissions =
		fs::metadata(&fake_gh_path).expect("fake gh metadata should read").permissions();

	#[cfg(unix)]
	PermissionsExt::set_mode(&mut permissions, 0o755);
	fs::set_permissions(&fake_gh_path, permissions)
		.expect("fake gh script should become executable");

	(fake_gh_path, invocation_log_path)
}

fn sample_issue(state_name: &str, labels: &[&str]) -> TrackerIssue {
	sample_issue_with_project_slug_and_sort_fields(
		"issue-1",
		"PUB-101",
		"pubfi",
		state_name,
		labels,
		Some(3),
		"2026-03-13T04:16:17.133Z",
	)
}

fn sample_blocker(id: &str, identifier: &str, state_name: &str) -> TrackerIssueBlocker {
	TrackerIssueBlocker {
		id: id.to_owned(),
		identifier: identifier.to_owned(),
		state: TrackerState { id: format!("state-{id}"), name: state_name.to_owned() },
	}
}

fn sample_issue_with_sort_fields(
	id: &str,
	identifier: &str,
	state_name: &str,
	labels: &[&str],
	priority: Option<i64>,
	created_at: &str,
) -> TrackerIssue {
	sample_issue_with_project_slug_and_sort_fields(
		id, identifier, "pubfi", state_name, labels, priority, created_at,
	)
}

fn sample_issue_with_project_slug_and_sort_fields(
	id: &str,
	identifier: &str,
	_project_slug: &str,
	state_name: &str,
	labels: &[&str],
	priority: Option<i64>,
	created_at: &str,
) -> TrackerIssue {
	let team_labels = vec![
		TrackerLabel {
			id: String::from("label-queued"),
			name: crate::tracker::automation_queue_label(_project_slug),
		},
		TrackerLabel {
			id: String::from("label-active"),
			name: crate::tracker::automation_active_label(_project_slug),
		},
		TrackerLabel {
			id: String::from("label-manual"),
			name: String::from("decodex:manual-only"),
		},
		TrackerLabel {
			id: String::from("label-needs-attention"),
			name: String::from("decodex:needs-attention"),
		},
	];

	TrackerIssue {
		id: id.to_owned(),
		identifier: identifier.to_owned(),
		#[cfg(test)]
		project_slug: Some(_project_slug.to_owned()),
		title: String::from("Implement orchestration"),
		author: Some(String::from("Yvette")),
		description: String::from("Body"),
		priority,
		created_at: created_at.to_owned(),
		updated_at: created_at.to_owned(),
		state: TrackerState { id: String::from("state-current"), name: state_name.to_owned() },
		team: TrackerTeam {
			id: String::from("team-1"),
			name: String::from("Pubfi"),
			states: vec![
				TrackerState { id: String::from("state-todo"), name: String::from("Todo") },
				TrackerState {
					id: String::from("state-progress"),
					name: String::from("In Progress"),
				},
				TrackerState { id: String::from("state-review"), name: String::from("In Review") },
			],
			labels: team_labels.clone(),
		},
		labels_complete: true,
		labels: labels
			.iter()
			.copied()
			.chain(iter::once(tracker::automation_queue_label(_project_slug).as_str()))
			.collect::<BTreeSet<_>>()
			.into_iter()
			.enumerate()
			.map(|(index, label)| TrackerLabel {
				id: format!("label-{index}"),
				name: label.to_owned(),
			})
			.collect(),
		blockers: Vec::new(),
	}
}

fn sample_issue_without_needs_attention_team_label(
	state_name: &str,
	labels: &[&str],
) -> TrackerIssue {
	let mut issue = sample_issue(state_name, labels);

	issue.team.labels.retain(|label| label.name != "decodex:needs-attention");

	issue
}

fn sample_review_handoff_marker(
	branch_name: &str,
	pr_url: &str,
	head_oid: &str,
) -> state::ReviewHandoffMarker {
	state::ReviewHandoffMarker::new("run-1", 1, branch_name, pr_url, "main", branch_name, head_oid)
}

fn seed_review_handoff_marker(
	state_store: &StateStore,
	project_id: &str,
	issue_id: &str,
	branch_name: &str,
	pr_url: &str,
	head_oid: &str,
) {
	state_store
		.upsert_review_handoff_marker(
			project_id,
			issue_id,
			&sample_review_handoff_marker(branch_name, pr_url, head_oid),
		)
		.expect("review handoff marker should persist");
}

fn seed_review_handoff_marker_value(
	state_store: &StateStore,
	project_id: &str,
	issue_id: &str,
	marker: &state::ReviewHandoffMarker,
) {
	state_store
		.upsert_review_handoff_marker(project_id, issue_id, marker)
		.expect("review handoff marker should persist");
}

fn seed_review_handoff_marker_for_path(
	state_store: &StateStore,
	project_id: &str,
	worktree_path: &Path,
	marker: &state::ReviewHandoffMarker,
) {
	let worktree = worktree_mapping_for_path(state_store, project_id, worktree_path);

	seed_review_handoff_marker_value(state_store, project_id, worktree.issue_id(), marker);
}

fn seed_review_orchestration_marker(
	state_store: &StateStore,
	project_id: &str,
	issue_id: &str,
	marker: &state::ReviewOrchestrationMarker,
) {
	state_store
		.upsert_review_handoff_marker(
			project_id,
			issue_id,
			&state::ReviewHandoffMarker::new(
				marker.run_id().to_owned(),
				marker.attempt_number(),
				marker.branch_name().to_owned(),
				marker.pr_url().to_owned(),
				"main",
				marker.branch_name().to_owned(),
				marker.head_sha().to_owned(),
			),
		)
		.expect("review handoff marker should persist");
	state_store
		.upsert_review_orchestration_marker(project_id, issue_id, marker)
		.expect("review orchestration marker should persist");
}

fn seed_review_orchestration_marker_for_path(
	state_store: &StateStore,
	project_id: &str,
	worktree_path: &Path,
	marker: &state::ReviewOrchestrationMarker,
) {
	let worktree = worktree_mapping_for_path(state_store, project_id, worktree_path);

	seed_review_orchestration_marker(state_store, project_id, worktree.issue_id(), marker);
}

fn persisted_review_handoff_marker(
	state_store: &StateStore,
	project_id: &str,
	issue_id: &str,
	branch_name: &str,
) -> state::ReviewHandoffMarker {
	state_store
		.review_handoff_marker(project_id, issue_id, branch_name)
		.expect("review handoff marker should read")
		.expect("review handoff marker should exist")
}

fn persisted_review_orchestration_marker(
	state_store: &StateStore,
	project_id: &str,
	issue_id: &str,
	branch_name: &str,
) -> state::ReviewOrchestrationMarker {
	let handoff = persisted_review_handoff_marker(state_store, project_id, issue_id, branch_name);

	state_store
		.review_orchestration_marker(project_id, issue_id, &handoff)
		.expect("review orchestration marker should read")
		.expect("review orchestration marker should exist")
}

fn persisted_review_orchestration_marker_for_path(
	state_store: &StateStore,
	project_id: &str,
	worktree_path: &Path,
) -> state::ReviewOrchestrationMarker {
	let worktree = worktree_mapping_for_path(state_store, project_id, worktree_path);

	persisted_review_orchestration_marker(
		state_store,
		project_id,
		worktree.issue_id(),
		worktree.branch_name(),
	)
}

fn worktree_mapping_for_path(
	state_store: &StateStore,
	project_id: &str,
	worktree_path: &Path,
) -> WorktreeMapping {
	state_store
		.list_worktrees(project_id)
		.expect("worktree list should read")
		.into_iter()
		.find(|worktree| worktree.worktree_path() == worktree_path)
		.expect("worktree mapping should exist for path")
}

fn sample_review_orchestration_marker(
	branch_name: &str,
	pr_url: &str,
	head_oid: &str,
	phase: &str,
	external_round_count: i64,
) -> state::ReviewOrchestrationMarker {
	state::ReviewOrchestrationMarker::new(
		"run-1",
		1,
		branch_name,
		pr_url,
		head_oid,
		phase,
		Some(TEST_EXTERNAL_REVIEW_REQUEST_COMMENT_ID),
		Some(TEST_EXTERNAL_REVIEW_REQUEST_CREATED_AT),
		Some(0),
		0,
		external_round_count,
		if phase == "waiting_for_merge" {
			Some(TEST_EXTERNAL_REVIEW_AUTO_MERGE_ENABLED_AT)
		} else {
			None
		},
	)
}

fn add_external_review_ack(review_state: &mut PullRequestReviewState) {
	add_review_request_ack_from_actor(review_state, EXTERNAL_REVIEW_ACTOR_LOGIN);
}

fn add_review_request_ack_from_actor(review_state: &mut PullRequestReviewState, actor_login: &str) {
	review_state.issue_comments.push(PullRequestIssueCommentState {
		database_id: TEST_EXTERNAL_REVIEW_REQUEST_COMMENT_ID,
		author_login: Some(actor_login.to_owned()),
		body: String::from(EXTERNAL_REVIEW_REQUEST_BODY),
		created_at_unix_epoch: TEST_EXTERNAL_REVIEW_REQUEST_CREATED_AT,
		external_review_eyes_reaction_count: usize::from(
			actor_login.eq_ignore_ascii_case(EXTERNAL_REVIEW_ACTOR_LOGIN),
		),
	});
}

fn add_external_review_summary(
	review_state: &mut PullRequestReviewState,
	body: &str,
	state: &str,
	submitted_at_unix_epoch: i64,
) {
	add_review_summary_from_actor(
		review_state,
		EXTERNAL_REVIEW_ACTOR_LOGIN,
		body,
		state,
		submitted_at_unix_epoch,
	);
}

fn add_review_summary_from_actor(
	review_state: &mut PullRequestReviewState,
	actor_login: &str,
	body: &str,
	state: &str,
	submitted_at_unix_epoch: i64,
) {
	review_state.reviews.push(PullRequestReviewSummaryState {
		author_login: Some(actor_login.to_owned()),
		body: body.to_owned(),
		state: state.to_owned(),
		submitted_at_unix_epoch,
	});
}

fn add_external_review_pass(review_state: &mut PullRequestReviewState) {
	add_external_review_pass_from_actor(review_state, EXTERNAL_REVIEW_ACTOR_LOGIN);
}

fn add_external_review_pass_from_actor(
	review_state: &mut PullRequestReviewState,
	actor_login: &str,
) {
	if actor_login.eq_ignore_ascii_case(EXTERNAL_REVIEW_ACTOR_LOGIN) {
		review_state.issue_description_external_review_thumbs_up_count += 1;
	}

	add_review_summary_from_actor(
		review_state,
		actor_login,
		EXTERNAL_REVIEW_PASS_PHRASE,
		"APPROVED",
		TEST_EXTERNAL_REVIEW_REQUEST_CREATED_AT + 1,
	);
}

fn add_external_review_findings(review_state: &mut PullRequestReviewState, body: &str) {
	add_review_summary_from_actor(
		review_state,
		EXTERNAL_REVIEW_ACTOR_LOGIN,
		body,
		"COMMENTED",
		TEST_EXTERNAL_REVIEW_REQUEST_CREATED_AT + 1,
	);
}

fn git_output(worktree_path: &Path, args: &[&str]) -> String {
	let output = Command::new("git")
		.arg("-c")
		.arg("core.hooksPath=/dev/null")
		.arg("-C")
		.arg(worktree_path)
		.args(args)
		.output()
		.expect("git command should run");

	assert!(
		output.status.success(),
		"git {} should succeed: {}",
		args.join(" "),
		String::from_utf8_lossy(&output.stderr),
	);

	String::from_utf8(output.stdout).expect("git output should be utf-8").trim().to_owned()
}

fn git_status_success(worktree_path: &Path, args: &[&str]) {
	let output = Command::new("git")
		.arg("-c")
		.arg("core.hooksPath=/dev/null")
		.arg("-C")
		.arg(worktree_path)
		.args(args)
		.output()
		.expect("git command should run");

	assert!(
		output.status.success(),
		"git {} should succeed: {}",
		args.join(" "),
		String::from_utf8_lossy(&output.stderr),
	);
}

fn commit_worktree_change(
	worktree_path: &Path,
	file_name: &str,
	contents: &str,
	message: &str,
) -> String {
	git_status_success(worktree_path, &["config", "user.name", "Decodex Tests"]);
	git_status_success(worktree_path, &["config", "user.email", "decodex-tests@example.com"]);

	let absolute_path = worktree_path.join(file_name);

	if let Some(parent) = absolute_path.parent() {
		fs::create_dir_all(parent).expect("worktree file parent should exist");
	}

	fs::write(absolute_path, contents).expect("worktree file should write");

	git_status_success(worktree_path, &["add", file_name]);
	git_status_success(worktree_path, &["commit", "-m", message]);

	git_output(worktree_path, &["rev-parse", "HEAD"])
}

#[allow(clippy::too_many_arguments)]
fn sample_pull_request_review_state(
	pr_url: &str,
	branch_name: &str,
	head_oid: &str,
	review_decision: Option<&str>,
	mergeable: &str,
	merge_state_status: &str,
	check_state: Option<&str>,
	unresolved_review_threads: usize,
) -> PullRequestReviewState {
	sample_pull_request_review_state_with_pending_requests(
		pr_url,
		branch_name,
		head_oid,
		review_decision,
		mergeable,
		merge_state_status,
		check_state,
		unresolved_review_threads,
		0,
	)
}

#[allow(clippy::too_many_arguments)]
fn sample_pull_request_review_state_with_pending_requests(
	pr_url: &str,
	branch_name: &str,
	head_oid: &str,
	review_decision: Option<&str>,
	mergeable: &str,
	merge_state_status: &str,
	check_state: Option<&str>,
	unresolved_review_threads: usize,
	pending_review_requests: usize,
) -> PullRequestReviewState {
	let head_repository_owner =
		github::parse_pull_request_url(pr_url).expect("pull request URL should parse").owner;

	PullRequestReviewState {
		url: pr_url.to_owned(),
		state: String::from("OPEN"),
		is_draft: false,
		review_decision: review_decision.map(str::to_owned),
		merge_commit_allowed: true,
		pending_review_requests,
		mergeable: mergeable.to_owned(),
		merge_state_status: merge_state_status.to_owned(),
		head_ref_name: branch_name.to_owned(),
		head_ref_oid: head_oid.to_owned(),
		merge_commit_oid: None,
		head_repository_name: Some(
			github::parse_pull_request_url(pr_url).expect("pull request URL should parse").repo,
		),
		head_repository_owner: Some(head_repository_owner),
		status_check_rollup_state: check_state.map(str::to_owned),
		unresolved_review_threads,
		issue_description_external_review_thumbs_up_count: 0,
		issue_comments: Vec::new(),
		reviews: Vec::new(),
	}
}

fn sample_pull_request_review_state_page(
	pr_url: &str,
	branch_name: &str,
	head_oid: &str,
	unresolved_review_threads: usize,
	has_next_page: bool,
	end_cursor: Option<&str>,
) -> PullRequestReviewStateNode {
	let locator = github::parse_pull_request_url(pr_url).expect("pull request URL should parse");

	PullRequestReviewStateNode {
		url: pr_url.to_owned(),
		state: String::from("OPEN"),
		is_draft: false,
		review_decision: Some(String::from("APPROVED")),
		review_requests: PullRequestReviewRequestConnection { total_count: 0 },
		mergeable: String::from("MERGEABLE"),
		merge_state_status: String::from("CLEAN"),
		head_ref_name: branch_name.to_owned(),
		head_ref_oid: head_oid.to_owned(),
		merge_commit: None,
		head_repository: Some(PullRequestRepository { name: locator.repo }),
		head_repository_owner: Some(PullRequestRepositoryOwner { login: locator.owner }),
		reaction_groups: Vec::new(),
		comments: PullRequestIssueCommentConnection {
			nodes: Vec::new(),
			page_info: PullRequestPageInfo { has_next_page: false, end_cursor: None },
		},
		reviews: PullRequestReviewConnection { nodes: Vec::new() },
		review_threads: PullRequestReviewThreadConnection {
			nodes: (0..unresolved_review_threads)
				.map(|_| PullRequestReviewThreadNode { is_resolved: false, is_outdated: false })
				.collect(),
			page_info: PullRequestPageInfo {
				has_next_page,
				end_cursor: end_cursor.map(str::to_owned),
			},
		},
		commits: PullRequestCommitConnection {
			nodes: vec![PullRequestCommitNode {
				commit: PullRequestCommitPayload {
					status_check_rollup: Some(PullRequestStatusCheckRollup {
						state: String::from("SUCCESS"),
					}),
				},
			}],
		},
	}
}

fn sample_pull_request_review_state_repository(
	pull_request: PullRequestReviewStateNode,
) -> PullRequestReviewStateRepository {
	PullRequestReviewStateRepository {
		merge_commit_allowed: true,
		pull_request: Some(pull_request),
	}
}

fn try_git_local_config_value(repo_root: &Path, key: &str) -> Option<String> {
	let output = Command::new("git")
		.arg("-C")
		.arg(repo_root)
		.args(["config", "--local", "--get", key])
		.output()
		.expect("git config should run");

	if !output.status.success() {
		return None;
	}

	Some(
		String::from_utf8(output.stdout)
			.expect("git config output should be utf-8")
			.trim()
			.to_owned(),
	)
}

fn git_remote_url(repo_root: &Path, remote_name: &str) -> Option<String> {
	let output = Command::new("git")
		.arg("-C")
		.arg(repo_root)
		.args(["remote", "get-url", remote_name])
		.output()
		.expect("git remote get-url should run");

	if !output.status.success() {
		return None;
	}

	Some(
		String::from_utf8(output.stdout)
			.expect("git remote get-url output should be utf-8")
			.trim()
			.to_owned(),
	)
}

fn temp_project_layout() -> (TempDir, ServiceConfig, WorkflowDocument) {
	temp_project_layout_with_tracker_project_slug_and_read_first(
		"pubfi",
		&[],
		"Follow the repository policy.\n",
	)
}

fn sample_workflow() -> WorkflowDocument {
	temp_project_layout().2
}

fn write_service_config(repo_root: &Path, contents: &str) {
	fs::create_dir_all(service_config_dir(repo_root)).expect("service config dir should exist");

	let contents =
		contents.replace("repo_root = \".\"", &format!("repo_root = \"{}\"", repo_root.display()));

	fs::write(service_config_path(repo_root), contents).expect("service config should write");
}

fn load_service_config(repo_root: &Path) -> ServiceConfig {
	ServiceConfig::from_path(service_config_path(repo_root)).expect("service config should load")
}

fn service_config_path(repo_root: &Path) -> PathBuf {
	service_config_dir(repo_root).join(TEST_PROJECT_CONFIG_FILE)
}

fn service_config_dir(repo_root: &Path) -> PathBuf {
	repo_root
		.parent()
		.expect("repo root should have temp parent")
		.join(".codex/decodex/projects/project")
}

fn service_workflow_path(repo_root: &Path) -> PathBuf {
	service_config_dir(repo_root).join("WORKFLOW.md")
}

fn sample_service_config_toml(
	service_id: &str,
	tracker_api_key_env_var: &str,
	github_token_env_var: &str,
	worktree_root: Option<&Path>,
	review_level: ReviewLevel,
) -> String {
	sample_service_config_toml_with_github_command_path(
		service_id,
		tracker_api_key_env_var,
		github_token_env_var,
		worktree_root,
		review_level,
		None,
	)
}

fn sample_service_config_toml_with_github_command_path(
	service_id: &str,
	tracker_api_key_env_var: &str,
	github_token_env_var: &str,
	worktree_root: Option<&Path>,
	review_level: ReviewLevel,
	github_command_path: Option<&Path>,
) -> String {
	let mut toml = format!(
		r#"service_id = "{service_id}"

[tracker]
api_key_env_var = "{tracker_api_key_env_var}"

[github]
token_env_var = "{github_token_env_var}"
"#
	);

	if let Some(github_command_path) = github_command_path {
		toml.push_str(&format!("command_path = \"{}\"\n", github_command_path.display()));
	}

	if review_level != ReviewLevel::Strict {
		toml.push_str("\n\n[codex]\n");
		toml.push_str(&format!("review = \"{}\"\n", review_level.as_str()));
	}

	toml.push_str(
		r#"

[paths]
repo_root = "."
"#,
	);

	if let Some(worktree_root) = worktree_root {
		toml.push_str(&format!("worktree_root = \"{}\"\n", worktree_root.display()));
	}

	toml
}

fn service_config_toml_for_config(
	config: &ServiceConfig,
	github_token_env_var: &str,
	review_level: ReviewLevel,
) -> String {
	service_config_toml_for_config_with_github_command_path(
		config,
		github_token_env_var,
		review_level,
		config.github().command_path(),
	)
}

fn service_config_toml_for_config_with_github_command_path(
	config: &ServiceConfig,
	github_token_env_var: &str,
	review_level: ReviewLevel,
	github_command_path: Option<&Path>,
) -> String {
	let default_worktree_root = config.repo_root().join(".worktrees");
	let worktree_root =
		(config.worktree_root() != default_worktree_root).then_some(config.worktree_root());

	sample_service_config_toml_with_github_command_path(
		config.service_id(),
		config.tracker().api_key_env_var(),
		github_token_env_var,
		worktree_root,
		review_level,
		github_command_path,
	)
}

fn service_config_with_github_token_env_var(
	config: &ServiceConfig,
	token_env_var: &str,
) -> ServiceConfig {
	write_service_config(
		config.repo_root(),
		&service_config_toml_for_config(config, token_env_var, config.codex().review_level()),
	);

	load_service_config(config.repo_root())
}

fn service_config_with_github_token_env_var_and_command_path(
	config: &ServiceConfig,
	token_env_var: &str,
	github_command_path: &Path,
) -> ServiceConfig {
	write_service_config(
		config.repo_root(),
		&service_config_toml_for_config_with_github_command_path(
			config,
			token_env_var,
			config.codex().review_level(),
			Some(github_command_path),
		),
	);

	load_service_config(config.repo_root())
}

fn service_config_with_review_level(
	config: &ServiceConfig,
	review_level: ReviewLevel,
) -> ServiceConfig {
	write_service_config(
		config.repo_root(),
		&service_config_toml_for_config_with_github_command_path(
			config,
			config.github().token_env_var(),
			review_level,
			config.github().command_path(),
		),
	);

	load_service_config(config.repo_root())
}

#[allow(dead_code)]
fn temp_project_layout_with_tracker_project_slug(
	_project_slug: &str,
) -> (TempDir, ServiceConfig, WorkflowDocument) {
	temp_project_layout_with_tracker_project_slug_and_read_first(
		"pubfi",
		&[],
		"Follow the repository policy.\n",
	)
}

fn temp_project_layout_with_read_first(
	read_first_files: &[(&str, &str)],
	workflow_body: &str,
) -> (TempDir, ServiceConfig, WorkflowDocument) {
	temp_project_layout_with_tracker_project_slug_and_read_first(
		"pubfi",
		read_first_files,
		workflow_body,
	)
}

fn temp_project_layout_with_max_turns(
	max_turns: u32,
) -> (TempDir, ServiceConfig, WorkflowDocument) {
	temp_project_layout_with_tracker_project_slug_max_turns_and_read_first(
		"pubfi",
		max_turns,
		&[],
		"Follow the repository policy.\n",
	)
}

fn temp_project_layout_with_tracker_project_slug_and_read_first(
	_project_slug: &str,
	read_first_files: &[(&str, &str)],
	workflow_body: &str,
) -> (TempDir, ServiceConfig, WorkflowDocument) {
	temp_project_layout_with_tracker_project_slug_max_turns_and_read_first(
		"pubfi",
		1,
		read_first_files,
		workflow_body,
	)
}

fn temp_project_layout_with_tracker_project_slug_max_turns_and_read_first(
	_project_slug: &str,
	max_turns: u32,
	read_first_files: &[(&str, &str)],
	workflow_body: &str,
) -> (TempDir, ServiceConfig, WorkflowDocument) {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let repo_root = temp_dir.path().join("target-repo");
	let read_first_paths = read_first_files.iter().map(|(path, _)| *path).collect::<Vec<_>>();

	fs::create_dir_all(&repo_root).expect("repo root should exist");
	fs::create_dir_all(repo_root.join(".worktrees")).expect("worktree root should exist");
	fs::create_dir_all(service_config_dir(&repo_root)).expect("service config dir should exist");

	for (relative_path, contents) in read_first_files {
		let absolute_path = repo_root.join(relative_path);

		if let Some(parent) = absolute_path.parent() {
			fs::create_dir_all(parent).expect("read_first parent should exist");
		}

		fs::write(absolute_path, contents).expect("read_first file should exist");
	}

	fs::write(
		service_workflow_path(&repo_root),
		sample_workflow_markdown("pubfi", &read_first_paths, workflow_body, max_turns),
	)
	.expect("workflow should exist");
	fs::write(repo_root.join("README.md"), "test repo\n").expect("tracked repo file should exist");

	write_service_config(
		&repo_root,
		&sample_service_config_toml("pubfi", "HOME", "HOME", None, ReviewLevel::Strict),
	);
	git_status_success(&repo_root, &["init", "-b", "main"]);
	git_status_success(&repo_root, &["config", "user.name", "Decodex Tests"]);
	git_status_success(&repo_root, &["config", "user.email", "decodex-tests@example.com"]);
	git_status_success(&repo_root, &["config", "commit.gpgsign", "false"]);
	git_status_success(&repo_root, &["add", "."]);
	git_status_success(&repo_root, &["commit", "-m", "bootstrap repo"]);

	let config = load_service_config(&repo_root);
	let workflow =
		WorkflowDocument::from_path(config.workflow_path()).expect("workflow should load");

	(temp_dir, config, workflow)
}

fn temp_project_layout_with_workflow_markdown(
	workflow_markdown: &str,
) -> (TempDir, ServiceConfig, WorkflowDocument) {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let repo_root = temp_dir.path().join("target-repo");

	fs::create_dir_all(&repo_root).expect("repo root should exist");
	fs::create_dir_all(repo_root.join(".worktrees")).expect("worktree root should exist");
	fs::create_dir_all(service_config_dir(&repo_root)).expect("service config dir should exist");
	fs::write(service_workflow_path(&repo_root), workflow_markdown).expect("workflow should exist");
	fs::write(repo_root.join("README.md"), "test repo\n").expect("tracked repo file should exist");

	write_service_config(
		&repo_root,
		&sample_service_config_toml("pubfi", "HOME", "HOME", None, ReviewLevel::Strict),
	);
	git_status_success(&repo_root, &["init", "-b", "main"]);
	git_status_success(&repo_root, &["config", "user.name", "Decodex Tests"]);
	git_status_success(&repo_root, &["config", "user.email", "decodex-tests@example.com"]);
	git_status_success(&repo_root, &["config", "commit.gpgsign", "false"]);
	git_status_success(&repo_root, &["add", "."]);
	git_status_success(&repo_root, &["commit", "-m", "bootstrap repo"]);

	let config = load_service_config(&repo_root);
	let workflow =
		WorkflowDocument::from_path(config.workflow_path()).expect("workflow should load");

	(temp_dir, config, workflow)
}

fn profile_scoped_workflow_markdown(project_slug: &str) -> String {
	let _ = project_slug;
	let markdown = r#"
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
max_turns = 1
max_retry_backoff_ms = 300000
canonicalize_commands = ["cargo make fmt", "cargo make lint-fix"]
verify_commands = ["cargo make check"]

[execution.gate_profiles.config_subset]
match_mode = "only"
paths = ["config/**"]
canonicalize_commands = []
verify_commands = ["python3 -c 'print(\"ok\")'"]

[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = []
timeout_seconds = 60

[context]
read_first = []
+++

Follow the repository policy.
"#;

	markdown.to_string()
}

fn add_origin_remote(repo_root: &Path, remote_root: &Path) {
	let remote_url = remote_root.display().to_string();

	git_status_success(
		remote_root.parent().expect("remote root should have parent"),
		&[
			"init",
			"--bare",
			"-b",
			"main",
			remote_root.to_str().expect("remote path should be utf-8"),
		],
	);
	git_status_success(repo_root, &["remote", "add", "origin", remote_url.as_str()]);
	git_status_success(repo_root, &["push", "-u", "origin", "main"]);
}

fn checkout_new_branch(repo_root: &Path, branch_name: &str) {
	git_status_success(repo_root, &["checkout", "-b", branch_name]);
}

fn sample_workflow_markdown(
	_project_slug: &str,
	read_first: &[&str],
	workflow_body: &str,
	max_turns: u32,
) -> String {
	let read_first =
		read_first.iter().map(|path| format!("\"{path}\"")).collect::<Vec<_>>().join(", ");
	let context = format!("[context]\nread_first = [{read_first}]");
	let markdown = format!(
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
max_turns = {max_turns}
max_retry_backoff_ms = 300000
gate_profiles = {{}}
canonicalize_commands = []
verify_commands = []

[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = []
timeout_seconds = 60

{context}
+++

{workflow_body}"#
	);

	markdown
}
