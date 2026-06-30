mod execution_architecture_recovery;
mod execution_failure;
mod execution_phase_goal;
mod lane_control;
mod lane_decision;
mod status_autonomy;
mod status_execution_programs;
mod status_ghost_lane_cleanup;
mod status_ghost_lane_evidence;
mod status_github_cli_authority;
mod status_history_ledger;
mod status_history_projection;
mod status_issue_metadata;
mod status_models;
mod status_process_liveness;
mod status_project_display;
mod status_queued_attention;
mod status_run_projection;
mod status_summary;
mod status_worktrees;
mod harness_improvement {
	use crate::orchestrator::{IssueRunPlan, Result, Serialize, StateStore, Value, records, state};

	include!("orchestrator/harness_improvement.rs");
}

pub(crate) use lane_control::{
	DEFAULT_STEER_RESULT_WAIT_TIMEOUT, LaneInspectRequest, LaneInterruptRequest, interrupt_lane,
	print_lane_inspect, steer_lane,
};

#[cfg(unix)]
use std::os::fd::AsRawFd;
use std::{
	cmp::Ordering,
	collections::{BTreeMap, BTreeSet, HashMap, HashSet},
	env,
	error::Error,
	fmt::{self, Display, Formatter},
	fs::{self, File},
	io::{ErrorKind, Read, Write},
	net::{SocketAddr, TcpListener, TcpStream},
	path::{Path, PathBuf},
	process::{Child, Command, ExitStatus, Stdio},
	slice,
	sync::{
		Arc, Mutex,
		mpsc::{self, Receiver, RecvTimeoutError, Sender},
	},
	thread::{self, JoinHandle},
	time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use color_eyre::Report;
use records::LinearExecutionEventRecord;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::Sha256;
use state::{ProjectLoopEvidenceSnapshot, ProtocolActivityEventSummary};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	agent,
	agent::REVIEW_POLICY_CONVERGENCE_BUDGET,
	default_branch_sync, git_credentials, maintenance, state, tracker,
	tracker::{privacy_classifier::PublicProjectionPrivacyClassifier, public_text},
};
use state::PrivateExecutionEvent;
#[rustfmt::skip]
use crate::{agent::{RUN_LEASE_IDLE_TIMEOUT, AppServerCapabilityPreflightFailure, AppServerDynamicToolFailure, AppServerHomePreflightFailure, AppServerPhaseGoalFailure, AppServerProcessEnv, AppServerRunRequest, AppServerRunResult, AppServerTransportFailure, AppServerTurnFailure, ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME, ISSUE_LABEL_ADD_TOOL_NAME, ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME, ISSUE_REVIEW_CHECKPOINT_TOOL_NAME, ISSUE_REVIEW_HANDOFF_TOOL_NAME, ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME, ISSUE_TERMINAL_FINALIZE_TOOL_NAME, ISSUE_TRANSITION_TOOL_NAME, DecodexRunContext, DecodexToolBridge, PhaseGoalController, PhaseGoalKind, PhaseGoalSpec, PhaseGoalTransition, ReviewExecutionMode, ReviewHandoffContext, ReviewHandoffWritebackFailed, ReviewPolicyStopReason, ReviewPolicyStopRequested, RunCompletionDisposition, TrackerToolBridge, TurnContinuationGuard}, config::{ReviewLevel, ServiceConfig}, execution_program::{ExecutionNodeEvaluation, ExecutionProgramEvaluation, ExecutionProgramOperatorSummary, ExecutionProgramReadinessContext, ExecutionWorkflowPolicy}, git_credentials::GitCredentialSource, github, prelude::{Result, eyre}, state::{ChildAgentActivityBucket, ChildAgentActivitySummary, CodexAccountActivitySummary, ExecutionProgramRecord, LoopGuardrailCheckpoint, LoopGuardrailCheckpointInput, ProjectRegistration, ProjectRunStatus, ProtocolActivitySummary, RUN_OPERATION_AGENT_RUN, RUN_OPERATION_GIT_CREDENTIALS, RUN_OPERATION_IDLE, RUN_OPERATION_RECONCILIATION, RUN_OPERATION_REPO_GATE, RUN_OPERATION_REVIEW_WRITEBACK, RUN_OPERATION_WAITING_EXTERNAL, ReviewHandoffMarker, ReviewOrchestrationMarker, RunActivityMarker, RunAttempt, StateStore, WorktreeMapping}, tracker::{IssueTracker, TrackerIssue, linear::LinearClient, records}, workflow::{WorkflowDocument, WorkflowExecution}, worktree::{WorktreeManager, WorktreeSpec}};
use execution_architecture_recovery::{
	architecture_recovery_retry_next_action, loop_guardrail_architecture_recovery_decision,
};
use execution_failure::{
	ARCHITECTURE_RECOVERY_BUDGET, ARCHITECTURE_RECOVERY_RETRY_KIND,
	AppServerZeroEvidenceStartFailure, ArchitectureRecoveryStart,
	LOOP_GUARDRAIL_CONVERGENCE_BUDGET, LoopGuardrailRecoveryDecision,
	RETRYABLE_FAILED_START_CLEANUP_EVENT_TYPE, RetainedReviewRepairPushFailed,
	RetainedReviewRepairPushFailureKind, TerminalFailureWritebackRuntime,
	apply_terminal_failure_writeback, ensure_automation_activity_label, git_guardrail_output,
	handle_failure, loop_guardrail_effective_status, loop_guardrail_text_hash,
	loop_guardrail_worktree_fingerprint, preserve_and_promote_app_server_run_failure,
	retained_progress_source_error_class, retryable_failure_loop_guardrail_stop,
	run_failure_requires_terminal_attention, run_failure_writeback_disposition,
	truncate_private_diagnostic_text,
};
#[cfg(test)]
use execution_failure::{
	RunFailureWritebackDisposition, preserve_manual_attention_request,
	promote_zero_evidence_app_server_start_failure, retry_budget_attempts_for_current_failure,
	write_retry_schedule_marker_for_runtime_retry,
};
#[cfg(test)]
use execution_phase_goal::RepoGatePhaseGoalController;
use execution_phase_goal::{
	PhaseAcceptanceCheckFailure, PhaseGoalRecoveryContinuation, build_phase_goal_controller,
	issue_has_blocking_lane_decision_evidence, latest_open_issue_phase_goal_before_attempt,
	maybe_continue_after_phase_goal_recovery, recover_phase_goal_continuation,
};
use harness_improvement::{HarnessOutcomeKind, record_harness_outcome_best_effort};
#[cfg(test)]
use harness_improvement::{HarnessOutcomeRecordInput, record_harness_outcome_for_issue_run};
use lane_decision::{
	LaneDecisionSnapshot, LaneNextAction, decide_lane_next_action,
	lane_decision_blocks_automatic_execution,
};
use status_autonomy::{
	operator_autonomy_lineage_statuses, operator_autonomy_objective_status,
	operator_autonomy_proposal_statuses, operator_autonomy_report_status,
	operator_autonomy_signal_statuses,
};
use status_execution_programs::operator_execution_program_statuses;
pub(crate) use status_ghost_lane_cleanup::ghost_lane_cleanup_status_blockers;
use status_ghost_lane_cleanup::{
	apply_missing_issue_ghost_lane_projection, mark_operator_run_tracker_issue_missing,
};
use status_github_cli_authority::{
	operator_github_cli_authority, operator_github_cli_authority_from_registration,
};
use status_history_ledger::{
	collect_history_ledger_records, compare_history_ledger_record_position,
	hydrate_history_lanes_from_linear_ledger, local_history_ledger_records,
	not_loaded_history_ledger_outcome, operator_history_ledger_outcome, parse_rfc3339_unix_epoch,
};
use status_history_projection::{
	apply_operator_lane_terminal_projection, apply_terminal_history_ledger_outcome_to_run,
	apply_terminal_history_ledger_outcomes, current_lane_has_authoritative_live_owner,
	current_lane_terminal_projection_from_local_ledger, history_lane_group_key,
	history_ledger_outcome_is_terminal, history_ledger_outcome_requires_attention,
	hydrate_history_lanes_from_local_ledger, suppress_terminal_attention_queue_echoes,
};
use status_issue_metadata::{
	fill_missing_history_lane_issue_metadata, fill_missing_run_issue_metadata,
	hydrate_operator_run_rows_from_tracker, operator_run_is_stale_terminal_local_residue,
	operator_run_tracker_issue_identifier_selector,
};
use status_models::{
	AccountActivityMode, ExternalReviewRequestCiGate, LiveOperatorStatusObserverContext,
	LiveOperatorStatusSnapshotOptions, MarkerProcessLiveness, OperatorExecutionProgramReadback,
	OperatorHistoryLedgerRecord, OperatorIssueDisplayMetadata, OperatorLaneControlProjection,
	OperatorLaneTerminalProjection, OperatorLifecycleMetricPhase,
	OperatorReviewCheckpointSummaryFields, OperatorRunAppServerState,
	OperatorRunLifecycleProjection, OperatorRunProtocolSummary, OperatorRunTiming,
	OperatorTerminalFinalizeProjection, PostReviewLaneBuildContext, PostReviewOrchestrationStatus,
	PostReviewReadbackDegradation, PostReviewRuntimeState, RetainedCloseoutPrMergeGate,
	RunIssueMetadataHydration, TrackerObserverOutcome, WorktreeOwnership,
};
pub(crate) use status_process_liveness::process_is_alive;
use status_process_liveness::{
	marker_process_is_alive, marker_process_liveness_for_marker, run_activity_idle_timeout,
	worktree_activity_marker_is_fresh,
};
use status_project_display::operator_project_display_name;
use status_queued_attention::{
	operator_authority_decision_request_status_from_event, operator_queued_issue_attention_status,
};
use status_run_projection::{
	format_optional_i64, format_optional_unix_timestamp, hydrate_current_lane_lifecycle_metrics,
	operator_boundary_policy_blocks_landing, operator_boundary_policy_requires_enhanced_evidence,
	operator_history_lanes, operator_loop_status_for_run,
	operator_protocol_activity_detail_is_public, operator_run_group_key,
	operator_run_issue_identifier_from_fields, operator_run_status,
};
use status_summary::{
	hydrate_post_review_lane_current_lane_shadowing, operator_issue_attention_key,
	operator_run_counts_as_attention, operator_run_counts_as_current_lane,
	operator_run_counts_as_running, operator_run_counts_as_waiting,
	operator_run_has_fresh_execution, operator_run_has_live_execution,
	operator_run_has_recent_app_server_execution, operator_run_needs_attention,
	project_attention_count, project_history_only_attention_count,
	queued_candidate_counts_as_waiting_intake, refresh_operator_project_summary,
};
pub(crate) use status_worktrees::ensure_project_has_no_merged_worktree_cleanup_debt;
use status_worktrees::{
	active_shared_issue_ids, operator_status_worktrees, refresh_worktree_ownership,
	stale_terminal_local_issue_ids,
};

mod types;
#[allow(unused_imports)]
pub(crate) use types::{
	ARCHITECTURE_RECOVERY_PACKET_EVENT_TYPE, ARCHITECTURE_RECOVERY_PACKET_SCHEMA,
	ARCHITECTURE_RECOVERY_STARTED_EVENT_TYPE, ARCHITECTURE_RECOVERY_TERMINAL_EVENT_TYPE,
	AUTHORITY_BOUNDARY_CHECK_EVENT_TYPE, AUTHORITY_DECISION_REQUEST_EVENT_TYPE,
	ActiveWorkflowOverride, AgentGitCredentialsUnavailable, AuthorityBoundaryChangedSurface,
	AuthorityBoundaryCheckInput, AuthorityBoundaryDisposition, AuthorityBoundaryImprovementSignal,
	AuthorityBoundaryPolicyDecision, AuthorityBoundarySurface, AuthorityDecisionOption,
	AuthorityDecisionRequestInput, CachedWorkflowDocument, ChildExitRetryContext, ChildRunRef,
	CurrentChildRunContext, DaemonRunChild, DaemonTickContext, DiagnoseRequest, EvidenceRequest,
	GhPullRequestReviewStateInspector, IssueDispatchMode, IssueRunPlan, IssueTurnContinuationGuard,
	LaneSteerReport, LaneSteerRequest, LoopGuardrailReason, LoopGuardrailStopRequested,
	ManualAttentionRequested, MaterializedDaemonSpawnState, OperatorArchitectureRecoveryStatus,
	OperatorAuthorityDecisionRequestStatus, OperatorAutonomyDecisionContractStatus,
	OperatorAutonomyExecutionEvidenceStatus, OperatorAutonomyLineageStatus,
	OperatorAutonomyObjectiveStatus, OperatorAutonomyProgramIntakeStatus,
	OperatorAutonomyProposalRefusalStatus, OperatorAutonomyProposalStatus,
	OperatorAutonomyReportReadbackStatus, OperatorAutonomySignalStatus, OperatorBoundaryStatus,
	OperatorCodexAccountControlStatus, OperatorConnectorBackoffStatus,
	OperatorContinuationRecoveryStatus, OperatorControlRequests,
	OperatorExecutionProgramNodeStatus, OperatorExecutionProgramStatus, OperatorGitHubCliAuthority,
	OperatorHistoryLaneStatus, OperatorHistoryLedgerOutcome, OperatorLaneLifecycleAttemptEvidence,
	OperatorLaneLifecycleMetrics, OperatorLaneLifecyclePhaseMetrics, OperatorLinearScanRequest,
	OperatorLoopStatus, OperatorPhaseAcceptanceStatus, OperatorPostReviewLaneStatus,
	OperatorProjectStatus, OperatorQueuedIssueAttentionStatus, OperatorQueuedIssueStatus,
	OperatorRecoveryBudgetStatus, OperatorReviewCheckpointStatus, OperatorReviewLoopStatus,
	OperatorReviewRouteCount, OperatorRunControlCapability, OperatorRunStatus,
	OperatorSnapshotWarningDetail, OperatorStateEndpoint, OperatorStatusSnapshot,
	OperatorWorktreeHygieneStatus, OperatorWorktreeProvenanceStatus, OperatorWorktreeStatus,
	PHASE_ACCEPTANCE_CHECK_EVENT_TYPE, PHASE_GOAL_RECOVERY_AUTOMATIC_CONTINUATION_LIMIT,
	PHASE_GOAL_RECOVERY_BLOCKED_EVENT_TYPE, PHASE_GOAL_RECOVERY_EVENT_TYPE,
	PostReviewLaneClassification, PostReviewLaneDecision, PostReviewLaneSnapshot,
	PostReviewLaneStateLoad, PreferredRunIdentity, PrepareIssueRunContext, ProjectDaemonRuntime,
	PublishedOperatorSnapshot, PullRequestActor, PullRequestCommitConnection,
	PullRequestCommitNode, PullRequestCommitPayload, PullRequestIssueCommentConnection,
	PullRequestIssueCommentNode, PullRequestIssueCommentState, PullRequestIssueCommentsData,
	PullRequestIssueCommentsNode, PullRequestIssueCommentsRepository,
	PullRequestIssueCommentsResponse, PullRequestMergeCommitNode, PullRequestPageInfo,
	PullRequestReactionGroup, PullRequestReactionUsersConnection, PullRequestReadbackFailure,
	PullRequestReadbackRootCause, PullRequestRepository, PullRequestRepositoryOwner,
	PullRequestReviewConnection, PullRequestReviewNode, PullRequestReviewRequestConnection,
	PullRequestReviewState, PullRequestReviewStateData, PullRequestReviewStateInspector,
	PullRequestReviewStateNode, PullRequestReviewStateRepository, PullRequestReviewStateResponse,
	PullRequestReviewSummaryState, PullRequestReviewThreadConnection, PullRequestReviewThreadNode,
	PullRequestStatusCheckRollup, RecoverableWorktreeSkipCache, RecoveredRuntimeState,
	RetainedPartialProgress, RetainedReviewLaneBlocked, RetainedReviewLaneLoad,
	RetainedReviewNeedsAttention, RetainedReviewRunIdentity, RetryDispatchDecision, RetryEntry,
	RetryIssueStateHint, RetryKind, RetryQueue, ReviewHandoffNeedsAttention,
	ReviewOrchestrationPhase, RunCycleRequest, RunLeaseDisposition, RunLeaseReconciliation,
	RunOnceRequest, RunSummary, SelectedIssueRunCandidate, ServeRequest, SpawnRunOnceChildRequest,
	StalledRunNeedsAttention, TargetIssueRunContext, TerminalFailureOutcome,
	TrackerConnectorBackoff, classify_pull_request_readback_report,
	record_authority_boundary_check_private_event, record_authority_decision_request_private_event,
};

include!("orchestrator/operator_presentation.rs");

include!("orchestrator/entrypoints.rs");

include!("orchestrator/operator_http.rs");

include!("orchestrator/pull_request_review.rs");

include!("orchestrator/program_reconciler.rs");

include!("orchestrator/daemon.rs");

include!("orchestrator/reconciliation.rs");

include!("orchestrator/retained_review_orchestration.rs");

include!("orchestrator/run_cycle.rs");

include!("orchestrator/runtime_validation.rs");

include!("orchestrator/execution_lifecycle.rs");

include!("orchestrator/execution.rs");

include!("orchestrator/dispatch_policy.rs");

include!("orchestrator/prompting.rs");

include!("orchestrator/git_ops.rs");

mod status;
#[allow(clippy::wildcard_imports)]
#[allow(unused_imports)]
use status::*;
pub(crate) use status::{worktree_checkout_branch_name, worktree_head_oid};

include!("orchestrator/status_render.rs");

include!("orchestrator/selection.rs");

mod agent_evidence;
#[cfg(test)]
use agent_evidence::PrivateEvidenceReadback;
use agent_evidence::{
	AgentEvidenceSource, AgentPrivateEvidenceRef, build_private_evidence_readback,
	private_evidence_ref_for_run_fields, render_agent_evidence_write_result,
	render_private_evidence_readback, render_private_evidence_reference,
	write_agent_evidence_best_effort, write_agent_evidence_snapshot,
};

pub(crate) const DEFAULT_STATUS_RUN_LIMIT: usize = 10;
pub(crate) const DEFAULT_OPERATOR_DASHBOARD_RUN_LIMIT: usize = 25;
pub(crate) const DEFAULT_OPERATOR_LISTEN_ADDRESS: &str = "127.0.0.1:8192";
pub(crate) const EXTERNAL_REVIEW_ACTOR_LOGIN: &str = "codex";
pub(crate) const EXTERNAL_REVIEW_REQUEST_BODY: &str = "@codex review";
pub(crate) const EXTERNAL_REVIEW_PASS_PHRASE: &str = "Didn't find any major issues.";
pub(crate) const EXTERNAL_REVIEW_ACK_TIMEOUT_SECS: i64 = 60;
pub(crate) const EXTERNAL_REVIEW_MERGE_VISIBILITY_TIMEOUT_SECS: i64 = 15 * 60;

const CONTINUATION_RETRY_DELAY_MS: u64 = 1_000;
const FAILURE_RETRY_BASE_DELAY_MS: u64 = 10_000;
const RECOVERABLE_WORKTREE_SKIP_TTL: Duration = Duration::from_secs(10 * 60);
const CONTINUATION_PENDING_RUN_STATUS: &str = "continuation_pending";
const TERMINAL_GUARDED_RUN_STATUS: &str = "terminal_guarded";
const TERMINAL_GUARD_MARKER_FILE: &str = ".decodex-terminal-guarded";
const TRACKER_RATE_LIMIT_BACKOFF_SECS: u64 = 15 * 60;
const TRACKER_RATE_LIMIT_WARNING: &str = "tracker_rate_limited";
const TRACKER_TRANSIENT_TIMEOUT_BACKOFF_SECS: u64 = 60;
const TRACKER_TRANSIENT_TIMEOUT_WARNING: &str = "tracker_transient_timeout";
const OPERATOR_DASHBOARD_ENDPOINT_PATH: &str = "/";
const OPERATOR_DASHBOARD_ALIAS_ENDPOINT_PATH: &str = "/dashboard";
const OPERATOR_DASHBOARD_WS_ENDPOINT_PATH: &str = "/dashboard/control";
const OPERATOR_LIVE_ENDPOINT_PATH: &str = "/livez";
const OPERATOR_ACCOUNTS_ENDPOINT_PATH: &str = "/api/accounts";
const OPERATOR_APP_SNAPSHOT_ENDPOINT_PATH: &str = "/api/operator-snapshot";
const OPERATOR_LINEAR_SCAN_ENDPOINT_PATH: &str = "/api/linear-scan";
const OPERATOR_LANE_INSPECT_ENDPOINT_PATH: &str = "/api/lane/inspect";
const OPERATOR_LANE_INTERRUPT_ENDPOINT_PATH: &str = "/api/lane/interrupt";
const OPERATOR_LANE_STEER_ENDPOINT_PATH: &str = "/api/lane/steer";
const OPERATOR_LANE_STEER_ALIAS_ENDPOINT_PATH: &str = "/api/lane-steer";
const OPERATOR_STATE_MAX_REQUEST_BYTES: usize = 256 * 1_024;
const OPERATOR_DASHBOARD_WS_CLIENT_MESSAGE_MAX_BYTES: usize = 64 * 1_024;
const OPERATOR_STATE_HEADER_TERMINATOR: &[u8] = b"\r\n\r\n";
const STATUS_OPERATOR_SNAPSHOT_MAX_AGE: Duration = Duration::from_secs(60);
const STATUS_OPERATOR_SNAPSHOT_CONNECT_TIMEOUT: Duration = Duration::from_millis(250);
const STATUS_OPERATOR_SNAPSHOT_IO_TIMEOUT: Duration = Duration::from_millis(500);
const STATUS_OPERATOR_SNAPSHOT_WARNING: &str = "status_cached_snapshot_unavailable";
const OPERATOR_DASHBOARD_WS_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(20);
const OPERATOR_RUN_ACTIVITY_STREAM_INTERVAL: Duration = Duration::from_secs(1);
const OPERATOR_DEV_SNAPSHOT_STREAM_INTERVAL: Duration = Duration::from_secs(1);
const DEFAULT_CONTROL_PLANE_POLL_INTERVAL: Duration = Duration::from_secs(15);
const LINEAR_CONTROL_PLANE_POLL_INTERVAL: Duration = Duration::from_secs(5 * 60);
const PULL_REQUEST_REVIEW_STATE_QUERY: &str = r#"
query($owner: String!, $name: String!, $number: Int!, $reviewThreadsAfter: String) {
  repository(owner: $owner, name: $name) {
    mergeCommitAllowed
    pullRequest(number: $number) {
      url
      state
      isDraft
      reviewDecision
      mergeable
      mergeStateStatus
      headRefName
      headRefOid
      mergeCommit {
        oid
      }
      headRepository {
        name
      }
      headRepositoryOwner {
        login
      }
      reactionGroups {
        content
        users(first: 100) {
          totalCount
          nodes {
            login
          }
        }
      }
      comments(first: 100) {
        nodes {
          databaseId
          body
          createdAt
          author {
            login
          }
          reactionGroups {
            content
            users(first: 100) {
              totalCount
              nodes {
                login
              }
            }
          }
        }
        pageInfo {
          hasNextPage
          endCursor
        }
      }
      reviews(last: 100) {
        nodes {
          body
          state
          submittedAt
          author {
            login
          }
        }
      }
      reviewRequests(first: 1) {
        totalCount
      }
      reviewThreads(first: 100, after: $reviewThreadsAfter) {
        nodes {
          isResolved
          isOutdated
        }
        pageInfo {
          hasNextPage
          endCursor
        }
      }
      commits(last: 1) {
        nodes {
          commit {
            statusCheckRollup {
              state
            }
          }
        }
      }
    }
  }
}
"#;
const PULL_REQUEST_ISSUE_COMMENTS_QUERY: &str = r#"
query($owner: String!, $name: String!, $number: Int!, $commentsAfter: String) {
  repository(owner: $owner, name: $name) {
    pullRequest(number: $number) {
      url
      comments(first: 100, after: $commentsAfter) {
        nodes {
          databaseId
          body
          createdAt
          author {
            login
          }
          reactionGroups {
            content
            users(first: 100) {
              totalCount
              nodes {
                login
              }
            }
          }
        }
        pageInfo {
          hasNextPage
          endCursor
        }
      }
    }
  }
}
"#;

#[cfg(test)]
mod tests;
