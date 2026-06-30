use std::collections::BTreeMap;
use tempfile::TempDir;
use time::OffsetDateTime;

use orchestrator::{
	AgentGitCredentialEnvironment, AgentGitCredentialsUnavailable,
	AppServerZeroEvidenceStartFailure, LoopGuardrailReason, LoopGuardrailStopRequested,
	RepoGateFailureKind, RunFailureWritebackDisposition, StalledRunNeedsAttention,
};

use crate::agent::CodexAccountAuthFailure;

use super::{
	AppServerCapabilityPreflightFailure, AppServerDynamicToolFailure,
	AppServerHomePreflightFailure, AppServerPhaseGoalFailure, AppServerTransportFailure,
	AppServerTurnFailure, AuthorityBoundaryChangedSurface, AuthorityBoundaryCheckInput,
	AuthorityBoundaryDisposition, AuthorityBoundaryPolicyDecision, AuthorityBoundarySurface,
	ChildRunRef, Command, Duration, FakeTracker, IssueDispatchMode, IssueRunPlan,
	ManualAttentionRequested, Path, PhaseGoalKind, PrepareIssueRunContext,
	RUN_ACTIVITY_MARKER_FILE, RUN_LEASE_IDLE_TIMEOUT, RUN_OPERATION_RECONCILIATION,
	RUN_OPERATION_REPO_GATE, Report, RetainedPartialProgress, RetainedReviewRepairPushFailed,
	RetryComment, ReviewHandoffMarker, ReviewPolicyCheckpointInput, ReviewPolicyStopReason,
	ReviewPolicyStopRequested, RunCompletionDisposition, ServiceConfig, StateStore,
	TEST_SERVICE_ID, TestEnvVarGuard, TrackerIssue, Value, WorktreeManager, WorktreeSpec,
	add_origin_remote, checkout_new_branch, commit_worktree_change, fs, git_output,
	git_status_success, orchestrator, process, sample_issue,
	service_config_with_github_token_env_var, state, temp_project_layout,
	temp_project_layout_with_read_first, tracker,
};

mod app_server;
mod comments;
mod handoff_recovery;
mod loop_guardrail;
mod retry_markers;
mod runtime_ops;

fn git_config_value(
	repo_root: &Path,
	key: &str,
	credentials: Option<&AgentGitCredentialEnvironment>,
) -> Option<String> {
	let mut probe = Command::new("git");

	probe.arg("-C").arg(repo_root).args(["config", "--get", key]);

	if let Some(credentials) = credentials {
		credentials.process_env().apply_to(&mut probe).expect("agent env should apply");
	}

	let output = probe.output().expect("git config probe should run");

	output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn injected_git_config_keys(credentials: &AgentGitCredentialEnvironment) -> Vec<String> {
	let mut probe = Command::new("git");

	credentials.process_env().apply_to(&mut probe).expect("agent env should apply");

	probe
		.get_envs()
		.filter_map(|(key, value)| {
			Some((key.to_string_lossy().into_owned(), value?.to_string_lossy().into_owned()))
		})
		.filter(|(key, _)| key.starts_with("GIT_CONFIG_KEY_"))
		.map(|(_, value)| value)
		.collect()
}

fn injected_git_config_values(credentials: &AgentGitCredentialEnvironment) -> Vec<String> {
	let mut probe = Command::new("git");

	credentials.process_env().apply_to(&mut probe).expect("agent env should apply");

	probe
		.get_envs()
		.filter_map(|(key, value)| {
			Some((key.to_string_lossy().into_owned(), value?.to_string_lossy().into_owned()))
		})
		.filter(|(key, _)| key.starts_with("GIT_CONFIG_VALUE_"))
		.map(|(_, value)| value)
		.collect()
}

pub(super) fn loop_guardrail_issue_run(
	config: &ServiceConfig,
	issue: &TrackerIssue,
	attempt_number: i64,
) -> IssueRunPlan {
	IssueRunPlan {
		issue: issue.clone(),
		issue_state: issue.state.name.clone(),
		initial_issue_state: issue.state.name.clone(),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: issue.identifier.clone(),
			path: config.repo_root().to_path_buf(),
			reused_existing: true,
		},
		retry_project_slug: issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: IssueDispatchMode::Normal,
		attempt_number,
		run_id: format!("pub-101-attempt-{attempt_number}-123"),
		retry_budget_base: 0,
	}
}

fn harness_outcome_payload_for_retryable_failure(error: Report) -> Value {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let tracker = FakeTracker::new(Vec::new());
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("In Progress", &[]);
	let issue_run = loop_guardrail_issue_run(&config, &issue, 1);

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");

	orchestrator::handle_failure(&tracker, &config, &workflow, &state_store, &issue_run, &error)
		.expect("retryable failure writeback should succeed");

	state_store
		.list_private_execution_events(
			config.service_id(),
			&issue.id,
			&issue_run.run_id,
			issue_run.attempt_number,
		)
		.expect("private evidence should list")
		.into_iter()
		.find(|event| event.event_type() == "harness_outcome")
		.expect("harness outcome should record")
		.payload()
		.clone()
}
