use super::{
	AgentPrivateEvidenceRef, Arc, BTreeSet, Child, ChildAgentActivityBucket,
	ChildAgentActivitySummary, CodexAccountActivitySummary, DashboardEventHub, Deserialize,
	Display, Duration, Error, ErrorKind, ExecutionNodeEvaluation, ExecutionProgramEvaluation,
	ExecutionProgramOperatorSummary, ExecutionProgramRecord, File, Formatter, HashMap, Instant,
	IssueTracker, JoinHandle, LinearClient, Mutex, OffsetDateTime, Path, PathBuf,
	ProtocolActivitySummary, PullRequestIssueCommentsPageQuery, PullRequestReviewStatePageQuery,
	RECOVERABLE_WORKTREE_SKIP_TTL, Report, Result, RetainedCloseoutPrMergeGate, RetainedReviewLane,
	ReviewHandoffMarker, RunAttempt, Sender, Serialize, ServiceConfig, SocketAddr, StateStore,
	TcpListener, TrackerIssue, TrackerToolBridge, TurnContinuationGuard, WorkflowDocument,
	WorktreeManager, WorktreeMapping, WorktreeSpec, eyre, fmt, github,
	issue_passes_closeout_dispatch_policy, issue_passes_dispatch_policy,
	issue_passes_retry_dispatch_policy, issue_passes_review_repair_dispatch_policy,
	issue_retry_budget_exhausted, json, merge_pull_request_issue_comment_page,
	merge_pull_request_review_state_page, mpsc, next_pull_request_issue_comments_cursor,
	next_pull_request_review_threads_cursor, operator_snapshot_json_value,
	ordinary_dispatch_blocked_by_retained_review_handoff, pull_request_review_state_from_page,
	query_pull_request_issue_comments_page, query_pull_request_review_state_page, refresh_issue,
	resolve_configured_env_var, retained_closeout_pr_merge_gate_with_inspector,
	run_operator_run_activity_websocket_broadcasts, run_operator_state_endpoint, state, thread,
};
use crate::tracker;

mod authority;
mod review_readback;

pub(crate) use authority::{
	ARCHITECTURE_RECOVERY_PACKET_EVENT_TYPE, ARCHITECTURE_RECOVERY_PACKET_SCHEMA,
	ARCHITECTURE_RECOVERY_STARTED_EVENT_TYPE, ARCHITECTURE_RECOVERY_TERMINAL_EVENT_TYPE,
	AUTHORITY_BOUNDARY_CHECK_EVENT_TYPE, AUTHORITY_DECISION_REQUEST_EVENT_TYPE,
	AuthorityBoundaryChangedSurface, AuthorityBoundaryCheckInput, AuthorityBoundaryDisposition,
	AuthorityBoundaryImprovementSignal, AuthorityBoundaryPolicyDecision, AuthorityBoundarySurface,
	AuthorityDecisionOption, AuthorityDecisionRequestInput, PHASE_ACCEPTANCE_CHECK_EVENT_TYPE,
	PHASE_GOAL_RECOVERY_AUTOMATIC_CONTINUATION_LIMIT, PHASE_GOAL_RECOVERY_BLOCKED_EVENT_TYPE,
	PHASE_GOAL_RECOVERY_EVENT_TYPE, record_authority_boundary_check_private_event,
	record_authority_decision_request_private_event,
};
pub(crate) use review_readback::{
	GhPullRequestReviewStateInspector, PullRequestActor, PullRequestCommitConnection,
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
	PullRequestStatusCheckRollup, classify_pull_request_readback_report,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum IssueDispatchMode {
	Normal,
	Program,
	Retry,
	ReviewRepair,
	Closeout,
}
impl IssueDispatchMode {
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::Normal => "normal",
			Self::Program => "program",
			Self::Retry => "retry",
			Self::ReviewRepair => "review_repair",
			Self::Closeout => "closeout",
		}
	}

	pub(crate) fn allows_issue(
		self,
		tracker: &dyn IssueTracker,
		issue: &TrackerIssue,
		project: &ServiceConfig,
		workflow: &WorkflowDocument,
		state_store: &StateStore,
		hint: RetryIssueStateHint<'_>,
	) -> crate::prelude::Result<bool> {
		match self {
			Self::Normal => {
				let queue_label = tracker::automation_queue_label(project.service_id());

				Ok(issue_passes_dispatch_policy(tracker, issue, workflow, &queue_label, false)?
					&& !ordinary_dispatch_blocked_by_retained_review_handoff(
						project.service_id(),
						issue,
						state_store,
					)?)
			},
			Self::Program => {
				let queue_label = tracker::automation_queue_label(project.service_id());

				Ok(issue_passes_dispatch_policy(tracker, issue, workflow, &queue_label, true)?
					&& !ordinary_dispatch_blocked_by_retained_review_handoff(
						project.service_id(),
						issue,
						state_store,
					)?)
			},
			Self::Retry => issue_passes_retry_dispatch_policy(
				tracker,
				issue,
				project,
				workflow,
				state_store,
				hint,
			),
			Self::ReviewRepair =>
				Ok(issue_passes_review_repair_dispatch_policy(tracker, issue, project, workflow)?
					&& !issue_retry_budget_exhausted(workflow, state_store, &issue.id)?),
			Self::Closeout => issue_passes_closeout_dispatch_policy(
				tracker,
				issue,
				project,
				workflow,
				state_store,
			),
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PostReviewLaneDecision {
	Continue,
	WaitForReview,
	NeedsReviewRepair,
	ReadyToLand,
	CloseoutBlocked,
	CleanupBlocked,
	Block,
}
impl PostReviewLaneDecision {
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::Continue => "continue",
			Self::WaitForReview => "wait_for_review",
			Self::NeedsReviewRepair => "needs_review_repair",
			Self::ReadyToLand => "ready_to_land",
			Self::CloseoutBlocked => "closeout_blocked",
			Self::CleanupBlocked => "cleanup_blocked",
			Self::Block => "blocked",
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RetryKind {
	Continuation,
	Failure,
}

pub(crate) enum RetryDispatchDecision {
	Blocked { excluded_issue_ids: Vec<String> },
	Dispatch(Box<RunSummary>),
	Continue,
}

#[derive(Clone, Debug)]
pub(crate) enum RunLeaseDisposition {
	RetainedReviewComplete,
	Superseded { newer_run_id: String, newer_attempt_number: i64 },
	Terminal,
	NotDispatchable,
	Stalled { idle_for: Duration },
	StalledRetainedPartialProgress { idle_for: Duration },
	StalledAlreadyNeedsAttention { idle_for: Duration },
}

pub(crate) enum RetainedReviewLaneLoad {
	Skip,
	Wait(String),
	Ready(Box<RetainedReviewLane>),
	Blocked(Box<RetainedReviewLaneBlocked>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReviewOrchestrationPhase {
	RequestPending,
	WaitingForAck,
	WaitingForResult,
	RepairRequired,
	PassWaitingForGates,
	WaitingForMerge,
}
impl ReviewOrchestrationPhase {
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::RequestPending => "request_pending",
			Self::WaitingForAck => "waiting_for_ack",
			Self::WaitingForResult => "waiting_for_result",
			Self::RepairRequired => "repair_required",
			Self::PassWaitingForGates => "pass_waiting_for_gates",
			Self::WaitingForMerge => "waiting_for_merge",
		}
	}

	pub(crate) fn parse(value: &str) -> std::result::Result<Self, String> {
		match value {
			"request_pending" => Ok(Self::RequestPending),
			"waiting_for_ack" => Ok(Self::WaitingForAck),
			"waiting_for_result" => Ok(Self::WaitingForResult),
			"repair_required" => Ok(Self::RepairRequired),
			"pass_waiting_for_gates" => Ok(Self::PassWaitingForGates),
			"waiting_for_merge" => Ok(Self::WaitingForMerge),
			other => Err(format!(
				"Unknown review orchestration phase `{other}` in retained review marker."
			)),
		}
	}
}

pub(crate) enum PostReviewLaneStateLoad {
	Classification(PostReviewLaneClassification),
	ReviewState(PullRequestReviewState),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LoopGuardrailReason {
	ValidationRepeat,
	NoEffectiveDiff,
	RemainingDeltaUnchanged,
	ReviewChurn,
	ReviewHandoffStateDrift,
	DependencyProgramStale,
	UncoveredDirection,
	AmbiguousRetainedProgress,
}
impl LoopGuardrailReason {
	pub(crate) fn error_class(self) -> &'static str {
		match self {
			Self::ValidationRepeat => "validation_repeat",
			Self::NoEffectiveDiff => "no_effective_diff",
			Self::RemainingDeltaUnchanged => "remaining_delta_unchanged",
			Self::ReviewChurn => "review_churn",
			Self::ReviewHandoffStateDrift => "review_handoff_state_drift",
			Self::DependencyProgramStale => "dependency_program_stale",
			Self::UncoveredDirection => "uncovered_direction",
			Self::AmbiguousRetainedProgress => "ambiguous_retained_progress",
		}
	}

	pub(crate) fn from_error_class(error_class: &str) -> Option<Self> {
		match error_class {
			"validation_repeat" | "validation_failure_repeated" => Some(Self::ValidationRepeat),
			"no_effective_diff" => Some(Self::NoEffectiveDiff),
			"remaining_delta_unchanged" => Some(Self::RemainingDeltaUnchanged),
			"review_churn" | "review_policy_exhausted" => Some(Self::ReviewChurn),
			"review_handoff_state_drift" | "review_handoff_rebind_required" =>
				Some(Self::ReviewHandoffStateDrift),
			"dependency_program_stale" | "dependency_blocked" => Some(Self::DependencyProgramStale),
			"uncovered_direction" | "research_contract_required" => Some(Self::UncoveredDirection),
			"ambiguous_retained_progress" | "ownership_ambiguous" =>
				Some(Self::AmbiguousRetainedProgress),
			_ => None,
		}
	}

	pub(crate) fn terminal_next_action(self, recovery_gate: &str) -> String {
		match self {
			Self::ValidationRepeat => format!(
				"inspect the repeated validation failure, preserved worktree, and prior repair attempts; change repair strategy or route the issue to architecture/research review manually, {recovery_gate}"
			),
			Self::NoEffectiveDiff => format!(
				"inspect the retained worktree and retry evidence; do not continue automatic repair until a human identifies a concrete next diff or resets the lane, {recovery_gate}"
			),
			Self::RemainingDeltaUnchanged => format!(
				"inspect the unchanged remaining delta and validation evidence; decide the next bounded repair manually before requeueing, {recovery_gate}"
			),
			Self::ReviewChurn => format!(
				"inspect the repeated review findings and current head; decide the next repair or architecture review manually before requeueing, {recovery_gate}"
			),
			Self::ReviewHandoffStateDrift => format!(
				"inspect the retained review handoff marker, clean review checkpoint, PR head, and issue state; restore or rebind the post-review lifecycle before clearing attention, {recovery_gate}"
			),
			Self::DependencyProgramStale => format!(
				"inspect the dependency blocker and Execution Program readiness evidence; refresh dependencies or split/research the program before requeueing, {recovery_gate}"
			),
			Self::UncoveredDirection => format!(
				"capture the missing direction in a research or decision contract before continuing execution, {recovery_gate}"
			),
			Self::AmbiguousRetainedProgress => format!(
				"inspect retained partial progress and ownership evidence; choose resume, reset, or manual repair explicitly before clearing the guard, {recovery_gate}"
			),
		}
	}
}

/// One bounded run invocation and its optional daemon-planned overrides.
pub(crate) struct RunOnceRequest<'a> {
	pub(crate) config_path: Option<&'a Path>,
	pub(crate) dry_run: bool,
	pub(crate) explain_queue: bool,
	pub(crate) preferred_issue_id: Option<&'a str>,
	pub(crate) preferred_issue_state: Option<&'a str>,
	pub(crate) preferred_initial_issue_state: Option<&'a str>,
	pub(crate) preferred_lease_acquired: bool,
	pub(crate) preferred_issue_claim_fd: Option<i32>,
	pub(crate) preferred_dispatch_slot_fd: Option<i32>,
	pub(crate) preferred_dispatch_slot_index: Option<usize>,
	pub(crate) preferred_dispatch_mode: Option<IssueDispatchMode>,
	pub(crate) preferred_run_id: Option<&'a str>,
	pub(crate) preferred_attempt_number: Option<i64>,
	pub(crate) preferred_retry_budget_base: Option<i64>,
	pub(crate) preferred_workflow_snapshot: Option<&'a str>,
}

/// Multi-project local control-plane daemon request.
pub(crate) struct ServeRequest<'a> {
	pub(crate) config_path: Option<&'a Path>,
	pub(crate) listen_address: &'a str,
	pub(crate) dev: bool,
}

/// Agent-readable runtime diagnosis request.
pub(crate) struct DiagnoseRequest<'a> {
	pub(crate) config_path: Option<&'a Path>,
	pub(crate) json: bool,
	pub(crate) limit: usize,
}

/// Local private execution evidence readback request.
pub(crate) struct EvidenceRequest<'a> {
	pub(crate) config_path: Option<&'a Path>,
	pub(crate) project_id: Option<&'a str>,
	pub(crate) issue: &'a str,
	pub(crate) run_id: Option<&'a str>,
	pub(crate) attempt_number: Option<i64>,
	pub(crate) json: bool,
	pub(crate) include_payload: bool,
}

/// Current lane steer request.
pub(crate) struct LaneSteerRequest<'a> {
	pub(crate) config_path: Option<&'a Path>,
	pub(crate) project_id: Option<&'a str>,
	pub(crate) issue: &'a str,
	pub(crate) run_id: &'a str,
	pub(crate) expected_turn_id: &'a str,
	pub(crate) message: &'a str,
	pub(crate) source: &'a str,
	pub(crate) wait_timeout: Duration,
}

/// Current lane steer result without raw operator message content.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LaneSteerReport {
	pub(crate) project_id: String,
	pub(crate) issue_id: String,
	pub(crate) issue_identifier: Option<String>,
	pub(crate) run_id: String,
	pub(crate) attempt_number: i64,
	pub(crate) thread_id: Option<String>,
	pub(crate) expected_turn_id: String,
	pub(crate) current_turn_id: Option<String>,
	pub(crate) response_turn_id: Option<String>,
	pub(crate) audit_record_id: i64,
	pub(crate) request_id: String,
	pub(crate) request_path: Option<String>,
	pub(crate) outcome: String,
	pub(crate) reason: String,
	pub(crate) failure_class: Option<String>,
	pub(crate) delivery_status: String,
	pub(crate) message_byte_count: usize,
	pub(crate) message_line_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RunSummary {
	pub(crate) project_id: String,
	pub(crate) issue_id: String,
	pub(crate) issue_identifier: String,
	pub(crate) issue_state: String,
	pub(crate) initial_issue_state: String,
	#[cfg(test)]
	pub(crate) retry_project_slug: String,
	pub(crate) dispatch_mode: IssueDispatchMode,
	pub(crate) branch_name: String,
	pub(crate) worktree_path: PathBuf,
	pub(crate) attempt_number: i64,
	pub(crate) run_id: String,
	pub(crate) continuation_pending: bool,
}

pub(crate) struct MaterializedDaemonSpawnState {
	pub(crate) worktree: WorktreeSpec,
	pub(crate) retry_budget_base: i64,
}

#[derive(Clone, Debug)]
pub(crate) struct IssueRunPlan {
	pub(crate) issue: TrackerIssue,
	pub(crate) issue_state: String,
	pub(crate) initial_issue_state: String,
	pub(crate) worktree: WorktreeSpec,
	#[allow(dead_code)]
	#[cfg(test)]
	pub(crate) retry_project_slug: String,
	pub(crate) dispatch_mode: IssueDispatchMode,
	pub(crate) attempt_number: i64,
	pub(crate) run_id: String,
	pub(crate) retry_budget_base: i64,
}

#[derive(Default)]
pub(crate) struct RecoveredRuntimeState {
	pub(crate) recoverable_issues: Vec<TrackerIssue>,
}

#[derive(Clone, Copy)]
pub(crate) struct RunCycleRequest<'a> {
	pub(crate) config_path: &'a Path,
	pub(crate) state_store: &'a StateStore,
	pub(crate) dry_run: bool,
	pub(crate) preferred_issue_id: Option<&'a str>,
	pub(crate) preferred_issue_state: Option<&'a str>,
	pub(crate) preferred_initial_issue_state: Option<&'a str>,
	pub(crate) preferred_lease_acquired: bool,
	pub(crate) preferred_issue_claim_fd: Option<i32>,
	pub(crate) preferred_dispatch_slot_fd: Option<i32>,
	pub(crate) preferred_dispatch_slot_index: Option<usize>,
	pub(crate) preferred_dispatch_mode: Option<IssueDispatchMode>,
	pub(crate) preferred_run_identity: Option<PreferredRunIdentity<'a>>,
	pub(crate) preferred_retry_budget_base: Option<i64>,
	pub(crate) preferred_workflow_snapshot: Option<&'a str>,
}

pub(crate) struct SpawnRunOnceChildRequest<'a> {
	pub(crate) config_path: &'a Path,
	pub(crate) preferred_issue_id: &'a str,
	pub(crate) preferred_issue_state: &'a str,
	pub(crate) preferred_initial_issue_state: Option<&'a str>,
	pub(crate) dispatch_mode: IssueDispatchMode,
	pub(crate) preferred_run_id: &'a str,
	pub(crate) preferred_attempt_number: i64,
	pub(crate) preferred_retry_budget_base: i64,
	pub(crate) workflow: &'a WorkflowDocument,
	pub(crate) issue_claim_handoff: Option<&'a File>,
	pub(crate) dispatch_slot_handoff: Option<&'a File>,
	pub(crate) dispatch_slot_index_handoff: Option<usize>,
}

#[derive(Clone, Copy)]
pub(crate) struct PrepareIssueRunContext<'a, T> {
	pub(crate) tracker: &'a T,
	pub(crate) project: &'a ServiceConfig,
	pub(crate) workflow: &'a WorkflowDocument,
	pub(crate) state_store: &'a StateStore,
	pub(crate) worktree_manager: &'a WorktreeManager,
	pub(crate) dry_run: bool,
	pub(crate) lease_preacquired: bool,
	pub(crate) dispatch_mode: IssueDispatchMode,
	pub(crate) preferred_issue_state: Option<&'a str>,
	pub(crate) preferred_initial_issue_state: Option<&'a str>,
	pub(crate) preferred_run_identity: Option<PreferredRunIdentity<'a>>,
	pub(crate) preferred_retry_budget_base: Option<i64>,
}

pub(crate) struct IssueTurnContinuationGuard<'a, T> {
	pub(crate) tracker: &'a T,
	pub(crate) tracker_tool_bridge: &'a TrackerToolBridge<'a>,
	pub(crate) workflow: &'a WorkflowDocument,
	pub(crate) service_id: &'a str,
	pub(crate) issue_id: &'a str,
	pub(crate) issue_identifier: &'a str,
	pub(crate) initial_issue_state: &'a str,
	#[allow(dead_code)]
	#[cfg(test)]
	pub(crate) retry_project_slug: &'a str,
	pub(crate) dispatch_mode: IssueDispatchMode,
	pub(crate) review_state_inspector: Option<&'a dyn PullRequestReviewStateInspector>,
}
impl<T> IssueTurnContinuationGuard<'_, T>
where
	T: IssueTracker,
{
	pub(crate) fn issue_has_service_ownership(
		&self,
		issue: &TrackerIssue,
	) -> crate::prelude::Result<bool> {
		tracker::issue_has_label_with_server_confirmation(
			self.tracker,
			issue,
			&tracker::automation_active_label(self.service_id),
		)
	}

	pub(crate) fn completed_closeout_pr_is_merged(&self) -> crate::prelude::Result<bool> {
		let Some(review_state_inspector) = self.review_state_inspector else {
			return Ok(false);
		};
		let Some(review_context) = self.tracker_tool_bridge.review_context() else {
			return Ok(false);
		};
		let Some(pr_url) = review_context.recorded_pr_url.as_deref() else {
			return Ok(false);
		};

		match retained_closeout_pr_merge_gate_with_inspector(
			&review_context.cwd,
			&review_context.branch_name,
			pr_url,
			review_state_inspector,
		)? {
			RetainedCloseoutPrMergeGate::Merged => Ok(true),
			RetainedCloseoutPrMergeGate::NotMerged => Ok(false),
			RetainedCloseoutPrMergeGate::PullRequestStateReadFailed => {
				eyre::bail!(
					"retained closeout PR state read failed while validating the continuation boundary"
				)
			},
		}
	}
}

impl<T> TurnContinuationGuard for IssueTurnContinuationGuard<'_, T>
where
	T: IssueTracker,
{
	fn should_continue_turn(&self, _turn_count: u32) -> crate::prelude::Result<bool> {
		let Some(issue) = refresh_issue(self.tracker, self.issue_id)? else {
			return Ok(false);
		};
		let tracker_policy = self.workflow.frontmatter().tracker();

		if !self.issue_has_service_ownership(&issue)? {
			return Ok(false);
		}
		if self.dispatch_mode == IssueDispatchMode::ReviewRepair {
			return Ok(issue.state.name == tracker_policy.success_state()
				&& !issue.has_label(tracker_policy.opt_out_label())
				&& !issue.has_label(tracker_policy.needs_attention_label()));
		}
		if self.dispatch_mode == IssueDispatchMode::Closeout {
			let completed_state = tracker_policy.resolved_completed_state();

			return Ok((issue.state.name == tracker_policy.success_state()
				|| (issue.state.name == completed_state
					&& self.completed_closeout_pr_is_merged()?))
				&& !issue.has_label(tracker_policy.opt_out_label())
				&& !issue.has_label(tracker_policy.needs_attention_label()));
		}

		let issue_remains_active = issue.state.name == tracker_policy.in_progress_state()
			&& !issue.has_label(tracker_policy.opt_out_label())
			&& !issue.has_label(tracker_policy.needs_attention_label());

		if issue_remains_active {
			return Ok(true);
		}

		let stale_startup_snapshot =
			self.tracker_tool_bridge.startup_transition_succeeded_locally()
				&& issue.state.name == self.initial_issue_state
				&& !issue.has_label(tracker_policy.opt_out_label())
				&& !issue.has_label(tracker_policy.needs_attention_label());

		Ok(stale_startup_snapshot)
	}

	fn validate_continuation_boundary(&self, turn_count: u32) -> crate::prelude::Result<()> {
		if self.dispatch_mode == IssueDispatchMode::ReviewRepair {
			let Some(issue) = refresh_issue(self.tracker, self.issue_id)? else {
				return Ok(());
			};
			let tracker_policy = self.workflow.frontmatter().tracker();

			if issue.state.name == tracker_policy.success_state()
				&& !issue.has_label(tracker_policy.opt_out_label())
				&& !issue.has_label(tracker_policy.needs_attention_label())
			{
				return Ok(());
			}

			eyre::bail!(
				"Turn {} for issue `{}` ended without keeping the tracker issue in `{}`; a clean {} continuation boundary is only valid while the lane remains in its retained post-review state.",
				turn_count,
				self.issue_identifier,
				tracker_policy.success_state(),
				"retained review-repair",
			);
		}
		if self.dispatch_mode == IssueDispatchMode::Closeout {
			let Some(issue) = refresh_issue(self.tracker, self.issue_id)? else {
				return Ok(());
			};
			let tracker_policy = self.workflow.frontmatter().tracker();
			let completed_state = tracker_policy.resolved_completed_state();
			let issue_completed_with_merged_pr =
				issue.state.name == completed_state && self.completed_closeout_pr_is_merged()?;

			if (issue.state.name == tracker_policy.success_state()
				|| issue_completed_with_merged_pr)
				&& !issue.has_label(tracker_policy.opt_out_label())
				&& !issue.has_label(tracker_policy.needs_attention_label())
			{
				return Ok(());
			}

			let retained_states =
				format!("`{}` or `{}`", tracker_policy.success_state(), completed_state);

			eyre::bail!(
				"Turn {} for issue `{}` ended without keeping the tracker issue in {}; a clean retained closeout continuation boundary is only valid while the lane remains in its retained post-review state.",
				turn_count,
				self.issue_identifier,
				retained_states,
			);
		}
		if turn_count != 1 {
			return Ok(());
		}
		if self.tracker_tool_bridge.startup_transition_succeeded_locally() {
			let Some(issue) = refresh_issue(self.tracker, self.issue_id)? else {
				return Ok(());
			};
			let tracker_policy = self.workflow.frontmatter().tracker();

			if !issue.has_label(tracker_policy.opt_out_label())
				&& !issue.has_label(tracker_policy.needs_attention_label())
				&& (issue.state.name == tracker_policy.in_progress_state()
					|| issue.state.name == self.initial_issue_state)
			{
				return Ok(());
			}
		}

		let Some(issue) = refresh_issue(self.tracker, self.issue_id)? else {
			return Ok(());
		};
		let in_progress = self.workflow.frontmatter().tracker().in_progress_state();

		if issue.state.name != in_progress {
			eyre::bail!(
				"Turn 1 for issue `{}` ended without moving the tracker issue to `{}`; a clean continuation boundary is only valid after the startup transition succeeds.",
				self.issue_identifier,
				in_progress
			);
		}

		Ok(())
	}
}

#[derive(Debug)]
pub(crate) struct ManualAttentionRequested {
	pub(crate) issue_identifier: String,
	pub(crate) label: String,
	pub(crate) run_id: String,
	pub(crate) error_class: Option<String>,
}
impl Display for ManualAttentionRequested {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		write!(
			f,
			"Run `{}` for issue `{}` requested human attention via label `{}`; stop automatic retries and hand off manually.",
			self.run_id, self.issue_identifier, self.label
		)
	}
}

impl Error for ManualAttentionRequested {}

#[derive(Debug)]
pub(crate) struct ReviewHandoffNeedsAttention {
	pub(crate) issue_identifier: String,
	pub(crate) pr_url: String,
	pub(crate) run_id: String,
}
impl Display for ReviewHandoffNeedsAttention {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		write!(
			f,
			"Run `{}` for issue `{}` partially applied review handoff writeback for PR `{}`; stop retries and repair the issue manually.",
			self.run_id, self.issue_identifier, self.pr_url
		)
	}
}

impl Error for ReviewHandoffNeedsAttention {}

#[derive(Debug)]
pub(crate) struct RetainedReviewNeedsAttention {
	pub(crate) reason: String,
}
impl Display for RetainedReviewNeedsAttention {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		write!(f, "Retained review orchestration requires operator attention: {}.", self.reason)
	}
}

impl Error for RetainedReviewNeedsAttention {}

#[derive(Debug)]
pub(crate) struct RetainedPartialProgress {
	pub(crate) issue_identifier: String,
	pub(crate) run_id: String,
	pub(crate) worktree_path: String,
	pub(crate) source_error_class: Option<String>,
}
impl Display for RetainedPartialProgress {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		write!(
			f,
			"Run `{}` for issue `{}` retained tracked worktree changes at `{}`; stop automatic retries and finish recovery manually.",
			self.run_id, self.issue_identifier, self.worktree_path
		)
	}
}

impl Error for RetainedPartialProgress {}

#[derive(Clone, Debug)]
pub(crate) struct LoopGuardrailStopRequested {
	pub(crate) issue_identifier: String,
	pub(crate) run_id: String,
	pub(crate) reason: LoopGuardrailReason,
	pub(crate) consecutive_count: i64,
	pub(crate) fingerprint: String,
	pub(crate) source_error_class: Option<String>,
	pub(crate) architecture_recovery_reason_code: Option<String>,
}
impl LoopGuardrailStopRequested {
	pub(crate) fn terminal_error_class(&self) -> &'static str {
		match self.architecture_recovery_reason_code.as_deref() {
			Some("architecture_recovery_exhausted") => "architecture_recovery_exhausted",
			Some("contract_boundary_required") => "contract_boundary_required",
			Some("external_dependency_required") => "external_dependency_required",
			Some("architecture_recovery_started") | None => self.reason.error_class(),
			Some(_) => self.reason.error_class(),
		}
	}

	pub(crate) fn terminal_next_action(&self, recovery_gate: &str) -> String {
		match self.architecture_recovery_reason_code.as_deref() {
			Some("architecture_recovery_exhausted") => format!(
				"inspect the Architecture Recovery Packet and prior recovery attempts; recovery budget is exhausted, {recovery_gate}"
			),
			Some("contract_boundary_required") => format!(
				"inspect the Authority Boundary Check and resolve the Decision Contract or authority evidence before retrying, {recovery_gate}"
			),
			Some("external_dependency_required") => format!(
				"inspect the dependency or Execution Program readiness blocker and resolve that external dependency before retrying, {recovery_gate}"
			),
			Some("architecture_recovery_started") | None =>
				self.reason.terminal_next_action(recovery_gate),
			Some(_) => self.reason.terminal_next_action(recovery_gate),
		}
	}
}

impl Display for LoopGuardrailStopRequested {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		let source = self.source_error_class.as_deref().unwrap_or("none");
		let architecture_recovery =
			self.architecture_recovery_reason_code.as_deref().unwrap_or("none");

		write!(
			f,
			"Run `{}` for issue `{}` hit loop guardrail `{}` after {} consecutive matching observations with source `{}` and fingerprint `{}`; architecture recovery reason `{}`.",
			self.run_id,
			self.issue_identifier,
			self.reason.error_class(),
			self.consecutive_count,
			source,
			self.fingerprint,
			architecture_recovery
		)
	}
}

impl Error for LoopGuardrailStopRequested {}

#[derive(Debug)]
pub(crate) struct AgentGitCredentialsUnavailable {
	pub(crate) run_id: String,
	pub(crate) token_env_var: String,
}
impl Display for AgentGitCredentialsUnavailable {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		write!(
			f,
			"Run `{}` could not prepare noninteractive GitHub credentials from `{}`; stop automatic execution and repair the configured credential.",
			self.run_id, self.token_env_var
		)
	}
}

impl Error for AgentGitCredentialsUnavailable {}

#[derive(Debug)]
pub(crate) struct StalledRunNeedsAttention {
	pub(crate) issue_identifier: String,
	pub(crate) run_id: String,
	pub(crate) idle_for: Duration,
}
impl Display for StalledRunNeedsAttention {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		write!(
			f,
			"Run `{}` for issue `{}` stalled after {:?} without app-server activity; reconcile through the retry budget before requiring operator attention.",
			self.run_id, self.issue_identifier, self.idle_for
		)
	}
}

impl Error for StalledRunNeedsAttention {}

pub(crate) struct DaemonRunChild {
	pub(crate) child: Child,
	pub(crate) issue_id: String,
	pub(crate) run_id: String,
	pub(crate) attempt_number: i64,
	pub(crate) initial_issue_state: String,
	#[allow(dead_code)]
	#[cfg(test)]
	pub(crate) retry_project_slug: String,
	pub(crate) dispatch_mode: IssueDispatchMode,
	pub(crate) from_retry_queue: bool,
	pub(crate) workflow: WorkflowDocument,
}

#[derive(Clone, Copy)]
pub(crate) struct ChildRunRef<'a> {
	pub(crate) issue_id: &'a str,
	pub(crate) run_id: &'a str,
	pub(crate) attempt_number: i64,
}

#[derive(Clone, Copy)]
pub(crate) struct CurrentChildRunContext<'a> {
	pub(crate) child: ChildRunRef<'a>,
	pub(crate) workflow: &'a WorkflowDocument,
	pub(crate) dispatch_mode: IssueDispatchMode,
}

#[derive(Clone, Copy)]
pub(crate) struct PreferredRunIdentity<'a> {
	pub(crate) run_id: &'a str,
	pub(crate) attempt_number: i64,
}

#[derive(Clone, Debug)]
pub(crate) struct RetryEntry {
	pub(crate) issue_id: String,
	#[allow(dead_code)]
	#[cfg(test)]
	pub(crate) retry_project_slug: String,
	pub(crate) continuation_initial_issue_state: Option<String>,
	pub(crate) dispatch_mode: IssueDispatchMode,
	pub(crate) kind: RetryKind,
	pub(crate) attempt: u32,
	pub(crate) ready_at: Instant,
}

#[derive(Default)]
pub(crate) struct RetryQueue {
	pub(crate) entries: HashMap<String, RetryEntry>,
}
impl RetryQueue {
	pub(crate) fn is_empty(&self) -> bool {
		self.entries.is_empty()
	}

	pub(crate) fn upsert(&mut self, entry: RetryEntry) {
		self.entries.insert(entry.issue_id.clone(), entry);
	}

	pub(crate) fn release(&mut self, issue_id: &str) {
		self.entries.remove(issue_id);
	}

	pub(crate) fn next_entry(&self) -> Option<&RetryEntry> {
		self.entries.values().min_by(|left, right| {
			left.ready_at.cmp(&right.ready_at).then_with(|| left.issue_id.cmp(&right.issue_id))
		})
	}

	pub(crate) fn ordered_entries(&self) -> Vec<RetryEntry> {
		let mut entries = self.entries.values().cloned().collect::<Vec<_>>();

		entries.sort_by(|left, right| {
			left.ready_at.cmp(&right.ready_at).then_with(|| left.issue_id.cmp(&right.issue_id))
		});

		entries
	}
}

#[derive(Default)]
pub(crate) struct RecoverableWorktreeSkipCache {
	pub(crate) entries: HashMap<String, Instant>,
}
impl RecoverableWorktreeSkipCache {
	pub(crate) fn is_suppressed(&mut self, issue_identifier: &str, now: Instant) -> bool {
		self.retain_active(now);

		self.entries.get(&issue_identifier.to_ascii_uppercase()).is_some_and(|until| *until > now)
	}

	pub(crate) fn remember(&mut self, issue_identifier: &str, now: Instant) {
		self.retain_active(now);
		self.entries
			.insert(issue_identifier.to_ascii_uppercase(), now + RECOVERABLE_WORKTREE_SKIP_TTL);
	}

	pub(crate) fn retain_active(&mut self, now: Instant) {
		self.entries.retain(|_, until| *until > now);
	}
}

pub(crate) struct DaemonTickContext {
	pub(crate) config: ServiceConfig,
	pub(crate) workflow: WorkflowDocument,
	pub(crate) tracker: LinearClient,
	pub(crate) worktree_manager: WorktreeManager,
}

#[derive(Default)]
pub(crate) struct ProjectDaemonRuntime {
	pub(crate) active_children: Vec<DaemonRunChild>,
	pub(crate) retry_queue: RetryQueue,
	pub(crate) tracker_backoff: Option<TrackerConnectorBackoff>,
	pub(crate) next_linear_scan_at: Option<Instant>,
	pub(crate) workflow_cache: Option<CachedWorkflowDocument>,
	pub(crate) recoverable_worktree_skip_cache: RecoverableWorktreeSkipCache,
}

#[derive(Clone, Debug)]
pub(crate) struct TrackerConnectorBackoff {
	pub(crate) until: Instant,
	pub(crate) quota_class: &'static str,
	pub(crate) reset_unix_epoch: i64,
	pub(crate) reset_source: &'static str,
	pub(crate) sync_phase: &'static str,
	pub(crate) warning: &'static str,
	pub(crate) next_action: &'static str,
}

pub(crate) struct OperatorStateEndpoint {
	pub(crate) listen_address: SocketAddr,
	pub(crate) snapshot: Arc<Mutex<PublishedOperatorSnapshot>>,
	pub(in crate::orchestrator) dashboard_events: DashboardEventHub,
	pub(crate) control_requests: OperatorControlRequests,
	pub(crate) shutdown_tx: Sender<()>,
	pub(crate) activity_shutdown_tx: Sender<()>,
	pub(crate) server_thread: Option<JoinHandle<()>>,
	pub(crate) activity_thread: Option<JoinHandle<()>>,
}
impl OperatorStateEndpoint {
	pub(crate) fn start(
		listen_address: &str,
		state_store: Arc<StateStore>,
	) -> crate::prelude::Result<Self> {
		let listener = TcpListener::bind(listen_address).map_err(|error| {
			eyre::eyre!("Failed to bind operator state endpoint on `{listen_address}`: {error}")
		})?;
		let listen_address = listener.local_addr().map_err(|error| {
			eyre::eyre!(
				"Failed to resolve operator state endpoint address for `{listen_address}`: {error}"
			)
		})?;

		listener
			.set_nonblocking(true)
			.map_err(|error| eyre::eyre!("Failed to configure operator state endpoint: {error}"))?;

		let snapshot = Arc::new(Mutex::new(PublishedOperatorSnapshot::default()));
		let dashboard_events = DashboardEventHub::default();
		let shared_snapshot = Arc::clone(&snapshot);
		let server_dashboard_events = dashboard_events.clone();
		let control_requests = OperatorControlRequests::default();
		let server_control_requests = control_requests.clone();
		let server_state_store = Arc::clone(&state_store);
		let (shutdown_tx, shutdown_rx) = mpsc::channel();
		let server_thread = thread::spawn(move || {
			run_operator_state_endpoint(
				listener,
				shared_snapshot,
				server_dashboard_events,
				server_control_requests,
				server_state_store,
				shutdown_rx,
			);
		});
		let activity_dashboard_events = dashboard_events.clone();
		let (activity_shutdown_tx, activity_shutdown_rx) = mpsc::channel();
		let activity_thread = thread::spawn(move || {
			run_operator_run_activity_websocket_broadcasts(
				state_store,
				activity_dashboard_events,
				activity_shutdown_rx,
			);
		});

		Ok(Self {
			listen_address,
			snapshot,
			dashboard_events,
			control_requests,
			shutdown_tx,
			activity_shutdown_tx,
			server_thread: Some(server_thread),
			activity_thread: Some(activity_thread),
		})
	}

	pub(crate) fn listen_address(&self) -> SocketAddr {
		self.listen_address
	}

	pub(crate) fn publish_snapshot(
		&self,
		snapshot: &OperatorStatusSnapshot,
	) -> crate::prelude::Result<()> {
		let snapshot_value = operator_snapshot_json_value(snapshot)?;
		let snapshot_json = serde_json::to_vec(&snapshot_value)?;
		let last_publish_unix_epoch = OffsetDateTime::now_utc().unix_timestamp();
		let mut guard = self
			.snapshot
			.lock()
			.map_err(|error| eyre::eyre!("Operator state snapshot lock poisoned: {error}"))?;

		*guard = PublishedOperatorSnapshot {
			snapshot_json: Some(snapshot_json),
			last_publish_unix_epoch: Some(last_publish_unix_epoch),
		};

		drop(guard);

		self.dashboard_events.broadcast(
			"snapshot",
			json!({
				"snapshotPublishedAtUnixEpoch": last_publish_unix_epoch,
				"snapshot": snapshot_value,
			}),
		);

		Ok(())
	}

	pub(crate) fn drain_linear_scan_requests(
		&self,
	) -> crate::prelude::Result<Vec<OperatorLinearScanRequest>> {
		self.control_requests.drain_linear_scan_requests()
	}
}

impl Drop for OperatorStateEndpoint {
	fn drop(&mut self) {
		let _ = self.shutdown_tx.send(());
		let _ = self.activity_shutdown_tx.send(());

		if let Some(server_thread) = self.server_thread.take() {
			let _ = server_thread.join();
		}
		if let Some(activity_thread) = self.activity_thread.take() {
			let _ = activity_thread.join();
		}
	}
}

#[derive(Clone, Default)]
pub(crate) struct PublishedOperatorSnapshot {
	pub(crate) snapshot_json: Option<Vec<u8>>,
	pub(crate) last_publish_unix_epoch: Option<i64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct OperatorLinearScanRequest {
	pub(crate) project_id: Option<String>,
}

#[derive(Clone, Default)]
pub(crate) struct OperatorControlRequests {
	pub(crate) linear_scan_requests: Arc<Mutex<Vec<OperatorLinearScanRequest>>>,
}
impl OperatorControlRequests {
	pub(crate) fn request_linear_scan(
		&self,
		project_id: Option<String>,
	) -> crate::prelude::Result<()> {
		let mut requests = self
			.linear_scan_requests
			.lock()
			.map_err(|error| eyre::eyre!("Operator control request lock poisoned: {error}"))?;

		requests.push(OperatorLinearScanRequest { project_id });

		Ok(())
	}

	pub(crate) fn drain_linear_scan_requests(
		&self,
	) -> crate::prelude::Result<Vec<OperatorLinearScanRequest>> {
		let mut requests = self
			.linear_scan_requests
			.lock()
			.map_err(|error| eyre::eyre!("Operator control request lock poisoned: {error}"))?;

		Ok(requests.drain(..).collect())
	}
}

#[derive(Clone)]
pub(crate) struct CachedWorkflowDocument {
	pub(crate) path: PathBuf,
	pub(crate) document: WorkflowDocument,
}

#[derive(Clone, Copy)]
pub(crate) struct ActiveWorkflowOverride<'a> {
	pub(crate) child: ChildRunRef<'a>,
	pub(crate) workflow: &'a WorkflowDocument,
}

#[derive(Clone, Debug)]
pub(crate) struct RunLeaseReconciliation {
	pub(crate) issue: TrackerIssue,
	pub(crate) run_attempt: RunAttempt,
	pub(crate) worktree_mapping: Option<WorktreeMapping>,
	pub(crate) disposition: RunLeaseDisposition,
	pub(crate) workflow: WorkflowDocument,
}

pub(crate) struct TerminalFailureOutcome {
	pub(crate) error_class: &'static str,
	pub(crate) retry_guarded_by_state: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct OperatorStatusSnapshot {
	pub(crate) project_id: String,
	pub(crate) run_limit: usize,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) status_source: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) snapshot_age_seconds: Option<i64>,
	pub(crate) warnings: Vec<String>,
	pub(crate) warning_details: Vec<OperatorSnapshotWarningDetail>,
	pub(crate) connector_backoffs: Vec<OperatorConnectorBackoffStatus>,
	pub(crate) projects: Vec<OperatorProjectStatus>,
	pub(crate) account_control: OperatorCodexAccountControlStatus,
	pub(crate) accounts: Vec<CodexAccountActivitySummary>,
	pub(crate) current_lanes: Vec<OperatorRunStatus>,
	pub(crate) recent_runs: Vec<OperatorRunStatus>,
	pub(crate) history_lanes: Vec<OperatorHistoryLaneStatus>,
	pub(crate) execution_programs: Vec<OperatorExecutionProgramStatus>,
	pub(crate) queued_candidates: Vec<OperatorQueuedIssueStatus>,
	pub(crate) worktrees: Vec<OperatorWorktreeStatus>,
	pub(crate) post_review_lanes: Vec<OperatorPostReviewLaneStatus>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorSnapshotWarningDetail {
	pub(crate) warning: String,
	pub(crate) project_id: Option<String>,
	pub(crate) repo_root: Option<String>,
	pub(crate) reason: String,
	pub(crate) next_action: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorConnectorBackoffStatus {
	pub(crate) project_id: String,
	pub(crate) connector: String,
	pub(crate) sync_phase: String,
	pub(crate) quota_class: String,
	pub(crate) reset_at: String,
	pub(crate) reset_unix_epoch: i64,
	pub(crate) reset_source: String,
	pub(crate) retry_after_seconds: i64,
	pub(crate) next_action: String,
	pub(crate) warning: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorProjectStatus {
	pub(crate) project_id: String,
	pub(crate) config_path: String,
	pub(crate) repo_root: String,
	pub(crate) enabled: bool,
	pub(crate) github_cli_authority: OperatorGitHubCliAuthority,
	pub(crate) current_lane_count: usize,
	pub(crate) running_lane_count: usize,
	pub(crate) queued_candidate_count: usize,
	pub(crate) post_review_lane_count: usize,
	pub(crate) retained_worktree_count: usize,
	pub(crate) waiting_lane_count: usize,
	pub(crate) attention_count: usize,
	pub(crate) cleanup_blocked_count: usize,
	pub(crate) cleanup_pending_count: usize,
	pub(crate) connector_state: String,
	pub(crate) last_activity_at: Option<String>,
	pub(crate) warning_count: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorExecutionProgramStatus {
	pub(crate) program_id: String,
	#[serde(default = "operator_execution_program_unknown_status")]
	pub(crate) status: String,
	pub(crate) source_contract_id: Option<String>,

	pub(crate) intake_kind: Option<String>,

	pub(crate) public_summary: Option<String>,
	pub(crate) node_count: usize,
	pub(crate) planned_count: usize,
	pub(crate) mapped_count: usize,
	pub(crate) ready_count: usize,
	pub(crate) queued_count: usize,
	pub(crate) blocked_count: usize,
	pub(crate) held_count: usize,
	pub(crate) active_count: usize,
	pub(crate) needs_attention_count: usize,
	pub(crate) completed_count: usize,
	pub(crate) stale_count: usize,
	pub(crate) superseded_count: usize,
	pub(crate) dispatchable_count: usize,
	pub(crate) mapped_issue_identifiers: Vec<String>,
	#[serde(default)]
	pub(crate) node_readbacks: Vec<OperatorExecutionProgramNodeStatus>,
	pub(crate) readback_warning: Option<String>,
}
impl OperatorExecutionProgramStatus {
	pub(crate) fn from_summary(
		record: &ExecutionProgramRecord,
		summary: ExecutionProgramOperatorSummary,
		evaluation: &ExecutionProgramEvaluation,
	) -> Self {
		let program_intake_plan = record.program().program_intake_plan();

		Self {
			status: operator_execution_program_status(
				&summary,
				record.program().nodes().len(),
				None,
			),
			program_id: summary.program_id.clone(),
			source_contract_id: record.source_contract_id().map(str::to_owned),
			intake_kind: program_intake_plan.map(|plan| plan.intake_kind().as_str().to_owned()),
			public_summary: program_intake_plan.map(|plan| plan.public_summary().to_owned()),
			node_count: record.program().nodes().len(),
			planned_count: summary.planned_count,
			mapped_count: summary.mapped_count,
			ready_count: summary.ready_count,
			queued_count: summary.queued_count,
			blocked_count: summary.blocked_count,
			held_count: summary.held_count,
			active_count: summary.active_count,
			needs_attention_count: summary.needs_attention_count,
			completed_count: summary.completed_count,
			stale_count: summary.stale_count,
			superseded_count: summary.superseded_count,
			dispatchable_count: summary.dispatchable_count,
			mapped_issue_identifiers: summary.mapped_issue_identifiers,
			node_readbacks: evaluation
				.nodes()
				.iter()
				.filter(|node| operator_execution_program_node_should_render(node))
				.map(operator_execution_program_node_readback)
				.collect(),
			readback_warning: None,
		}
	}

	pub(crate) fn missing_contract(record: &ExecutionProgramRecord) -> Self {
		let node_count = record.program().nodes().len();

		Self {
			program_id: record.program_id().to_owned(),
			status: String::from("stale"),
			source_contract_id: record.source_contract_id().map(str::to_owned),
			intake_kind: record
				.program()
				.program_intake_plan()
				.map(|plan| plan.intake_kind().as_str().to_owned()),
			public_summary: record
				.program()
				.program_intake_plan()
				.map(|plan| plan.public_summary().to_owned()),
			node_count,
			planned_count: 0,
			mapped_count: 0,
			ready_count: 0,
			queued_count: 0,
			blocked_count: 0,
			held_count: 0,
			active_count: 0,
			needs_attention_count: 0,
			completed_count: 0,
			stale_count: node_count,
			superseded_count: 0,
			dispatchable_count: 0,
			mapped_issue_identifiers: operator_execution_program_mapped_issue_identifiers(record),
			node_readbacks: operator_execution_program_missing_contract_nodes(record),
			readback_warning: Some(String::from("source_decision_contract_missing")),
		}
	}
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorExecutionProgramNodeStatus {
	#[serde(default = "operator_execution_program_unknown_status")]
	pub(crate) program_stage: String,
	pub(crate) lifecycle_state: String,
	pub(crate) readiness_state: String,
	pub(crate) issue_identifier: Option<String>,
	pub(crate) issue_state: Option<String>,
	pub(crate) dispatch_action: Option<String>,
	pub(crate) reason_codes: Vec<String>,
	pub(crate) reasons: Vec<String>,
	pub(crate) next_action: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorGitHubCliAuthority {
	pub(crate) command_path: String,
	pub(crate) resolved_path: Option<String>,
	pub(crate) configured_path: Option<String>,
	pub(crate) discovery_tier: String,
	pub(crate) available: bool,
	pub(crate) next_action: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorCodexAccountControlStatus {
	pub(crate) mode: String,
	pub(crate) account_selector: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorHistoryLaneStatus {
	pub(crate) project_id: String,
	pub(crate) issue_id: String,
	pub(crate) issue_identifier: Option<String>,
	pub(crate) title: Option<String>,
	pub(crate) author: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) issue_state: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) active_label_present: Option<bool>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) needs_attention_label_present: Option<bool>,
	pub(crate) issue_key: String,
	pub(crate) attempt_count: usize,
	pub(crate) ledger_outcome: OperatorHistoryLedgerOutcome,
	pub(crate) lifecycle_metrics: OperatorLaneLifecycleMetrics,
	pub(crate) latest_run: OperatorRunStatus,
	pub(crate) attempts: Vec<OperatorRunStatus>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorLaneLifecycleMetrics {
	pub(crate) attempt_count: usize,
	pub(crate) run_count: usize,
	pub(crate) recorded_attempt_count: usize,
	pub(crate) recovered_attempt_count: usize,
	pub(crate) current_snapshot_attempt_count: usize,
	pub(crate) captured_attempt_count: usize,
	pub(crate) missing_attempt_count: usize,
	pub(crate) protocol_event_count: i64,
	pub(crate) child_event_count: i64,
	pub(crate) wall_seconds: i64,
	pub(crate) tool_call_count: i64,
	pub(crate) input_tokens_current: Option<i64>,
	pub(crate) input_tokens_peak: Option<i64>,
	pub(crate) input_tokens_cumulative: i64,
	pub(crate) output_tokens_cumulative: i64,
	pub(crate) largest_tool_output_bytes: Option<i64>,
	pub(crate) largest_tool_output_tool: Option<String>,
	pub(crate) large_output_warnings: Vec<String>,
	pub(crate) buckets: Vec<ChildAgentActivityBucket>,
	pub(crate) phases: Vec<OperatorLaneLifecyclePhaseMetrics>,
	pub(crate) attempt_evidence: Vec<OperatorLaneLifecycleAttemptEvidence>,
	pub(crate) recovery_gaps: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorLaneLifecyclePhaseMetrics {
	pub(crate) phase: String,
	pub(crate) label: String,
	pub(crate) attempt_count: usize,
	pub(crate) run_count: usize,
	pub(crate) recorded_attempt_count: usize,
	pub(crate) recovered_attempt_count: usize,
	pub(crate) current_snapshot_attempt_count: usize,
	pub(crate) captured_attempt_count: usize,
	pub(crate) missing_attempt_count: usize,
	pub(crate) protocol_event_count: i64,
	pub(crate) child_event_count: i64,
	pub(crate) wall_seconds: i64,
	pub(crate) tool_call_count: i64,
	pub(crate) input_tokens_current: Option<i64>,
	pub(crate) input_tokens_peak: Option<i64>,
	pub(crate) input_tokens_cumulative: i64,
	pub(crate) output_tokens_cumulative: i64,
	pub(crate) largest_tool_output_bytes: Option<i64>,
	pub(crate) largest_tool_output_tool: Option<String>,
	pub(crate) large_output_warnings: Vec<String>,
	pub(crate) buckets: Vec<ChildAgentActivityBucket>,
	pub(crate) attempt_evidence: Vec<OperatorLaneLifecycleAttemptEvidence>,
	pub(crate) recovery_gaps: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorLaneLifecycleAttemptEvidence {
	pub(crate) run_id: String,
	pub(crate) issue_id: String,
	pub(crate) attempt_number: i64,
	pub(crate) status: String,
	pub(crate) phase: String,
	pub(crate) source: String,
	pub(crate) evidence: Vec<String>,
	pub(crate) gaps: Vec<String>,
	pub(crate) protocol_event_count: i64,
	pub(crate) child_event_count: i64,
	pub(crate) updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorHistoryLedgerOutcome {
	pub(crate) ledger_status: String,
	pub(crate) final_outcome: String,
	pub(crate) final_event_type: Option<String>,
	pub(crate) final_event_at: Option<String>,
	pub(crate) summary: Option<String>,
	pub(crate) pr_url: Option<String>,
	pub(crate) commit_sha: Option<String>,
	pub(crate) branch: Option<String>,
	pub(crate) closeout_status: Option<String>,
	pub(crate) needs_attention_reason: Option<String>,
	pub(crate) lifecycle_started_at: Option<String>,
	pub(crate) lifecycle_finished_at: Option<String>,
	pub(crate) lifecycle_elapsed_seconds: Option<i64>,
	pub(crate) record_count: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorRunStatus {
	pub(crate) project_id: String,
	pub(crate) project_display_name: String,
	pub(crate) run_id: String,
	pub(crate) issue_id: String,
	pub(crate) issue_identifier: Option<String>,
	pub(crate) title: Option<String>,
	pub(crate) author: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) issue_state: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) active_label_present: Option<bool>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) needs_attention_label_present: Option<bool>,
	pub(crate) attempt_number: i64,
	pub(crate) status: String,
	pub(crate) attempt_status: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) status_projection_reason: Option<String>,
	pub(crate) ownership_state: String,
	pub(crate) liveness_state: String,
	pub(crate) policy_state: String,
	pub(crate) terminalization_state: String,
	pub(crate) lane_control_next_action: String,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(crate) lane_control_conditions: Vec<String>,
	pub(crate) phase: String,
	#[serde(default)]
	pub(crate) run_phase: String,
	pub(crate) wait_reason: Option<String>,
	pub(crate) current_operation: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) active_goal_phase: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) public_progress_phase: Option<String>,
	pub(crate) thread_id: Option<String>,
	pub(crate) turn_id: Option<String>,
	pub(crate) thread_status: Option<String>,
	pub(crate) thread_active_flags: Vec<String>,
	pub(crate) interactive_requested: bool,
	pub(crate) continuation_pending: bool,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) continuation_recovery: Option<OperatorContinuationRecoveryStatus>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) phase_acceptance: Option<OperatorPhaseAcceptanceStatus>,
	pub(crate) run_lease: bool,
	pub(crate) queue_lease_state: String,
	pub(crate) execution_liveness: String,
	pub(crate) has_fresh_execution: bool,
	pub(crate) counts_as_running: bool,
	pub(crate) needs_attention: bool,
	pub(crate) updated_at: String,
	pub(crate) last_run_activity_at: Option<String>,
	pub(crate) last_protocol_activity_at: Option<String>,
	pub(crate) last_progress_at: Option<String>,
	pub(crate) idle_for_seconds: Option<i64>,
	pub(crate) protocol_idle_for_seconds: Option<i64>,
	pub(crate) suspected_stall: bool,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) progress_diagnostic: Option<String>,
	pub(crate) last_event_type: Option<String>,
	pub(crate) last_event_at: Option<String>,
	pub(crate) event_count: i64,
	pub(in crate::orchestrator) private_evidence: AgentPrivateEvidenceRef,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) loop_status: Option<OperatorLoopStatus>,
	pub(crate) control_capability: Option<OperatorRunControlCapability>,
	pub(crate) process_id: Option<u32>,
	pub(crate) process_alive: Option<bool>,
	pub(crate) process_liveness_reason: Option<String>,
	pub(crate) retry_kind: Option<String>,
	pub(crate) next_retry_at: Option<String>,
	pub(crate) effective_model: Option<String>,
	pub(crate) effective_model_provider: Option<String>,
	pub(crate) effective_cwd: Option<String>,
	pub(crate) effective_approval_policy: Option<String>,
	pub(crate) effective_approvals_reviewer: Option<String>,
	pub(crate) effective_sandbox_mode: Option<String>,
	pub(crate) child_agent_activity: Option<ChildAgentActivitySummary>,
	pub(crate) protocol_activity: Option<ProtocolActivitySummary>,
	pub(crate) lifecycle_source: String,
	pub(crate) lifecycle_evidence: Vec<String>,
	pub(crate) lifecycle_gaps: Vec<String>,
	#[serde(default)]
	pub(crate) lifecycle_metrics: OperatorLaneLifecycleMetrics,
	pub(crate) account: Option<CodexAccountActivitySummary>,
	pub(crate) accounts: Vec<CodexAccountActivitySummary>,
	pub(crate) branch_name: Option<String>,
	pub(crate) worktree_path: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorContinuationRecoveryStatus {
	pub(crate) state: String,
	pub(crate) source_phase: String,
	pub(crate) next_phase: String,
	pub(crate) source_error_class: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) source_error_message: Option<String>,
	pub(crate) recorded_at: String,
	pub(crate) run_id: String,
	pub(crate) attempt_number: i64,
	pub(crate) recovery_count: i64,
	pub(crate) automatic_continuation_limit: i64,
	pub(crate) budget_exceeded: bool,
	pub(crate) next_action: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorPhaseAcceptanceStatus {
	pub(crate) phase: String,
	pub(crate) decision: String,
	pub(crate) reason_code: String,
	pub(crate) objective_covered: bool,
	pub(crate) effective_delta_present: bool,
	pub(crate) changed_surfaces: Vec<String>,
	pub(crate) non_goal_passed: bool,
	pub(crate) validation_passed: bool,
	pub(crate) recorded_at: String,
	pub(crate) run_id: String,
	pub(crate) attempt_number: i64,
	pub(crate) next_action: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorRunControlCapability {
	pub(crate) project_id: String,
	pub(crate) issue_id: String,
	pub(crate) run_id: String,
	pub(crate) attempt_number: i64,
	pub(crate) thread_id: Option<String>,
	pub(crate) turn_id: Option<String>,
	pub(crate) transport: String,
	pub(crate) channel_path: String,
	pub(crate) status: String,
	pub(crate) published_at: String,
	pub(crate) updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorQueuedIssueStatus {
	pub(crate) project_id: String,
	pub(crate) issue_id: String,
	pub(crate) issue_identifier: String,
	pub(crate) title: String,
	pub(crate) author: Option<String>,
	pub(crate) state: String,
	pub(crate) priority: Option<i64>,
	pub(crate) created_at: String,
	pub(crate) classification: String,
	pub(crate) reason: String,
	pub(crate) attention: Option<OperatorQueuedIssueAttentionStatus>,
	pub(crate) blocker_identifiers: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorQueuedIssueAttentionStatus {
	pub(crate) summary: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) decision_request: Option<OperatorAuthorityDecisionRequestStatus>,
	pub(crate) run_id: Option<String>,
	pub(crate) attempt_number: Option<i64>,
	pub(crate) current_operation: Option<String>,
	pub(crate) thread_status: Option<String>,
	pub(crate) attempt_status: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) loop_status: Option<OperatorLoopStatus>,
	pub(crate) auto_retry_blocked_reason: Option<String>,
	pub(crate) attention_error_class: Option<String>,
	pub(crate) attention_next_action: Option<String>,
	pub(crate) retry_budget_attempt_count: Option<i64>,
	pub(crate) retry_budget_max_attempts: i64,
	pub(crate) last_activity_at: Option<String>,
	pub(crate) last_progress_at: Option<String>,
	pub(crate) last_event_type: Option<String>,
	pub(crate) event_count: i64,
	pub(crate) process_alive: Option<bool>,
	pub(crate) process_liveness_reason: Option<String>,
	pub(crate) worktree_path: Option<String>,
	pub(crate) worktree_has_tracked_changes: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorAuthorityDecisionRequestStatus {
	pub(crate) phase: String,
	pub(crate) reason: String,
	pub(crate) boundary: String,
	pub(crate) decision_request_id: String,
	pub(crate) next_action: String,
	pub(crate) recommendation: Option<String>,
	pub(crate) resume_condition: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorLoopStatus {
	pub(crate) review_level: String,
	pub(crate) autonomy: String,
	pub(crate) summary: String,
	pub(crate) next_action: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) autonomy_objective: Option<OperatorAutonomyObjectiveStatus>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(crate) autonomy_signals: Vec<OperatorAutonomySignalStatus>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(crate) autonomy_proposals: Vec<OperatorAutonomyProposalStatus>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(crate) autonomy_lineage: Vec<OperatorAutonomyLineageStatus>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) autonomy_report: Option<OperatorAutonomyReportReadbackStatus>,
	pub(crate) review: Option<OperatorReviewLoopStatus>,
	pub(crate) architecture_recovery: Option<OperatorArchitectureRecoveryStatus>,
	pub(crate) boundary: Option<OperatorBoundaryStatus>,
	pub(crate) decision_request: Option<OperatorAuthorityDecisionRequestStatus>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorAutonomyObjectiveStatus {
	pub(crate) objective_id: String,
	pub(crate) objective_version: u64,
	pub(crate) state: String,
	pub(crate) summary: String,
	pub(crate) source_ref: String,
	pub(crate) updated_at: String,
	pub(crate) completeness: String,
	pub(crate) known_gaps: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorAutonomySignalStatus {
	pub(crate) signal_id: String,
	pub(crate) objective_id: String,
	pub(crate) objective_version: u64,
	pub(crate) kind: String,
	pub(crate) source_type: String,
	pub(crate) source_refs: Vec<String>,
	pub(crate) primary_source_refs: Vec<String>,
	pub(crate) freshness: String,
	pub(crate) evidence_class: String,
	pub(crate) confidence: String,
	pub(crate) privacy: String,
	pub(crate) redaction_level: String,
	pub(crate) completeness: String,
	pub(crate) gaps: Vec<String>,
	pub(crate) known_gaps: Vec<String>,
	pub(crate) contradictions: Vec<String>,
	pub(crate) updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorAutonomyProposalStatus {
	pub(crate) proposal_id: String,
	pub(crate) objective_id: String,
	pub(crate) objective_version: u64,
	pub(crate) state: String,
	pub(crate) summary: String,
	pub(crate) source_family: String,
	pub(crate) intended_surface: String,
	pub(crate) affected_identifiers: Vec<String>,
	pub(crate) source_signal_ids: Vec<String>,
	pub(crate) refusal_reasons: Vec<String>,
	pub(crate) refusals: Vec<OperatorAutonomyProposalRefusalStatus>,
	pub(crate) completeness: String,
	pub(crate) known_gaps: Vec<String>,
	pub(crate) gaps: Vec<String>,
	pub(crate) contradictions: Vec<String>,
	pub(crate) challenge_evidence_count: usize,
	pub(crate) updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorAutonomyProposalRefusalStatus {
	pub(crate) reason: String,
	pub(crate) detail: String,
	pub(crate) evidence_refs: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorAutonomyLineageStatus {
	pub(crate) objective_ref: String,
	pub(crate) signal_ids: Vec<String>,
	pub(crate) proposal_id: Option<String>,
	pub(crate) proposal_state: Option<String>,
	pub(crate) decision_contracts: Vec<OperatorAutonomyDecisionContractStatus>,
	pub(crate) program_intake: Vec<OperatorAutonomyProgramIntakeStatus>,
	pub(crate) execution_evidence: Vec<OperatorAutonomyExecutionEvidenceStatus>,
	pub(crate) completeness: String,
	pub(crate) known_gaps: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorAutonomyDecisionContractStatus {
	pub(crate) contract_id: String,
	pub(crate) status: String,
	pub(crate) updated_at: String,
	pub(crate) generated_issue_identifiers: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorAutonomyProgramIntakeStatus {
	pub(crate) program_id: String,
	pub(crate) plan_id: String,
	pub(crate) intake_kind: String,
	pub(crate) source_contract_id: String,
	pub(crate) public_summary: String,
	pub(crate) updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorAutonomyExecutionEvidenceStatus {
	pub(crate) kind: String,
	pub(crate) issue_identifier: Option<String>,
	pub(crate) source_refs: Vec<String>,
	pub(crate) summary: String,
	pub(crate) updated_at: String,
	pub(crate) completeness: String,
	pub(crate) known_gaps: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorAutonomyReportReadbackStatus {
	pub(crate) surface: String,
	pub(crate) authority: String,
	pub(crate) audit_authority: bool,
	pub(crate) source_refs: Vec<String>,
	pub(crate) redaction_level: String,
	pub(crate) completeness: String,
	pub(crate) known_gaps: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorReviewLoopStatus {
	pub(crate) phase: String,
	pub(crate) status: String,
	pub(crate) checkpoint: Option<OperatorReviewCheckpointStatus>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorReviewCheckpointStatus {
	pub(crate) head_sha: String,
	pub(crate) round: i64,
	pub(crate) nonclean_rounds: i64,
	pub(crate) review_class: Option<String>,
	pub(crate) risk_class: Option<String>,
	pub(crate) compact_eligible: Option<bool>,
	pub(crate) fallback_reason: Option<String>,
	pub(crate) active_fingerprints: Vec<String>,
	pub(crate) stop_fingerprint: Option<String>,
	pub(crate) route_counts: Vec<OperatorReviewRouteCount>,
	pub(crate) route_next_action: Option<String>,
	pub(crate) updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorReviewRouteCount {
	pub(crate) route: String,
	pub(crate) count: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorArchitectureRecoveryStatus {
	pub(crate) status: String,
	pub(crate) reason_code: String,
	pub(crate) guardrail_reason: Option<String>,
	pub(crate) boundary_disposition: Option<String>,
	pub(crate) boundary_policy_decision: Option<String>,
	pub(crate) requires_enhanced_evidence: bool,
	pub(crate) blocks_landing: bool,
	pub(crate) round: Option<u64>,
	pub(crate) budget: Option<OperatorRecoveryBudgetStatus>,
	pub(crate) next_action: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorRecoveryBudgetStatus {
	pub(crate) attempt: u64,
	pub(crate) max_attempts: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorBoundaryStatus {
	pub(crate) disposition: String,
	pub(crate) policy_decision: String,
	pub(crate) reason: Option<String>,
	pub(crate) attempted_recovery_reason: Option<String>,
	pub(crate) changed_surface_count: usize,
	pub(crate) improvement_signal_count: usize,
	pub(crate) requires_enhanced_evidence: bool,
	pub(crate) blocks_landing: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorWorktreeStatus {
	pub(crate) project_id: String,
	pub(crate) issue_id: String,
	pub(crate) issue_identifier: Option<String>,
	pub(crate) issue_state: Option<String>,
	pub(crate) branch_name: String,
	pub(crate) worktree_path: String,
	pub(crate) ownership: String,
	pub(crate) ownership_reason: String,
	pub(crate) provenance: OperatorWorktreeProvenanceStatus,
	pub(crate) recovery_next_action: Option<String>,
	pub(crate) hygiene: Option<OperatorWorktreeHygieneStatus>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorWorktreeProvenanceStatus {
	pub(crate) source: String,
	pub(crate) created_at_unix: Option<i64>,
	pub(crate) updated_at_unix: Option<i64>,
	pub(crate) audit_required: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorWorktreeHygieneStatus {
	pub(crate) classification: String,
	pub(crate) default_branch: String,
	pub(crate) dirty: bool,
	pub(crate) reason: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorPostReviewLaneStatus {
	pub(crate) project_id: String,
	pub(crate) issue_id: String,
	pub(crate) issue_identifier: String,
	pub(crate) issue_state: String,
	pub(crate) branch_name: String,
	pub(crate) worktree_path: String,
	pub(crate) classification: String,
	pub(crate) reason: String,
	pub(crate) pr_url: Option<String>,
	pub(crate) pr_head_sha: Option<String>,
	pub(crate) pr_state: Option<String>,
	pub(crate) review_decision: Option<String>,
	pub(crate) mergeable: Option<String>,
	pub(crate) check_state: Option<String>,
	pub(crate) unresolved_review_threads: Option<usize>,
	pub(crate) shadowed_by_current_lane: bool,
	pub(crate) readback_warning: Option<String>,
	pub(crate) readback_root_cause: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) loop_status: Option<OperatorLoopStatus>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PostReviewLaneSnapshot {
	pub(crate) issue: TrackerIssue,
	pub(crate) worktree: WorktreeMapping,
	pub(crate) review_handoff: Option<ReviewHandoffMarker>,
	pub(crate) local_branch_name: Option<String>,
	pub(crate) local_head_oid: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PostReviewLaneClassification {
	pub(crate) decision: PostReviewLaneDecision,
	pub(crate) reason: String,
	pub(crate) pr_url: Option<String>,
	pub(crate) pr_head_sha: Option<String>,
	pub(crate) pr_state: Option<String>,
	pub(crate) review_decision: Option<String>,
	pub(crate) mergeable: Option<String>,
	pub(crate) check_state: Option<String>,
	pub(crate) unresolved_review_threads: Option<usize>,
	pub(crate) readback_warning: Option<String>,
	pub(crate) readback_root_cause: Option<String>,
}

pub(crate) struct RetainedReviewLaneBlocked {
	pub(crate) issue: TrackerIssue,
	pub(crate) worktree: WorktreeMapping,
	pub(crate) run_identity: RetainedReviewRunIdentity,
	pub(crate) reason: String,
}

pub(crate) struct RetainedReviewRunIdentity {
	pub(crate) run_id: String,
	pub(crate) attempt_number: i64,
}

pub(crate) struct SelectedIssueRunCandidate {
	pub(crate) issue: TrackerIssue,
	pub(crate) dispatch_mode: IssueDispatchMode,
	pub(crate) preferred_run_identity: Option<RetainedReviewRunIdentity>,
}
impl SelectedIssueRunCandidate {
	pub(crate) fn new(issue: TrackerIssue, dispatch_mode: IssueDispatchMode) -> Self {
		Self { issue, dispatch_mode, preferred_run_identity: None }
	}
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct RetryIssueStateHint<'a> {
	pub(crate) preferred_issue_state: Option<&'a str>,
	pub(crate) preferred_initial_issue_state: Option<&'a str>,
}

pub(crate) struct ChildExitRetryContext<'a, T> {
	pub(crate) retry_queue: &'a mut RetryQueue,
	pub(crate) tracker: &'a T,
	pub(crate) project: &'a ServiceConfig,
	pub(crate) workflow: &'a WorkflowDocument,
	pub(crate) state_store: &'a StateStore,
}

#[derive(Clone, Copy)]
pub(crate) struct TargetIssueRunContext<'a, T> {
	pub(crate) tracker: &'a T,
	pub(crate) project: &'a ServiceConfig,
	pub(crate) workflow: &'a WorkflowDocument,
	pub(crate) state_store: &'a StateStore,
	pub(crate) issue_id: &'a str,
	pub(crate) preferred_issue_state: Option<&'a str>,
	pub(crate) preferred_initial_issue_state: Option<&'a str>,
	pub(crate) dry_run: bool,
	pub(crate) lease_preacquired: bool,
	pub(crate) preferred_issue_claim_fd: Option<i32>,
	pub(crate) preferred_dispatch_slot_fd: Option<i32>,
	pub(crate) preferred_dispatch_slot_index: Option<usize>,
	pub(crate) dispatch_mode: IssueDispatchMode,
	pub(crate) preferred_run_identity: Option<PreferredRunIdentity<'a>>,
	pub(crate) preferred_retry_budget_base: Option<i64>,
}

pub(crate) fn operator_execution_program_status(
	summary: &ExecutionProgramOperatorSummary,
	node_count: usize,
	readback_warning: Option<&str>,
) -> String {
	if readback_warning.is_some() || summary.stale_count > 0 || summary.superseded_count > 0 {
		String::from("stale")
	} else if summary.needs_attention_count > 0 {
		String::from("attention")
	} else if summary.blocked_count > 0 {
		String::from("blocked")
	} else if summary.active_count > 0 {
		String::from("active")
	} else if summary.queued_count > 0 {
		String::from("queued")
	} else if summary.ready_count > 0 {
		String::from("ready")
	} else if node_count > 0 && summary.completed_count == node_count {
		String::from("completed")
	} else if summary.held_count > 0 {
		String::from("held")
	} else {
		String::from("idle")
	}
}

pub(crate) fn operator_execution_program_unknown_status() -> String {
	String::from("unknown")
}

pub(crate) fn operator_execution_program_node_should_render(
	node: &ExecutionNodeEvaluation,
) -> bool {
	node.dispatch_action().is_some()
		|| matches!(
			node.lifecycle_state(),
			crate::execution_program::ExecutionProgramNodeLifecycleState::Active
				| crate::execution_program::ExecutionProgramNodeLifecycleState::Blocked
				| crate::execution_program::ExecutionProgramNodeLifecycleState::Mapped
				| crate::execution_program::ExecutionProgramNodeLifecycleState::NeedsAttention
				| crate::execution_program::ExecutionProgramNodeLifecycleState::PostReview
				| crate::execution_program::ExecutionProgramNodeLifecycleState::Planned
				| crate::execution_program::ExecutionProgramNodeLifecycleState::Stale
				| crate::execution_program::ExecutionProgramNodeLifecycleState::Superseded
		)
}

pub(crate) fn operator_execution_program_node_readback(
	node: &ExecutionNodeEvaluation,
) -> OperatorExecutionProgramNodeStatus {
	let reason_codes = operator_execution_program_reason_codes(node.reasons());
	let reasons = node
		.reasons()
		.iter()
		.map(|reason| operator_execution_program_public_reason(reason))
		.collect::<Vec<_>>();
	let issue = node.linear_issue();

	OperatorExecutionProgramNodeStatus {
		program_stage: node.stage().as_str().to_owned(),
		lifecycle_state: node.lifecycle_state().as_str().to_owned(),
		readiness_state: node.state().as_str().to_owned(),
		issue_identifier: issue.map(|issue| issue.issue_identifier().to_owned()),
		issue_state: issue.map(|issue| issue.issue_state().to_owned()),
		dispatch_action: node.dispatch_action().map(|action| action.as_str().to_owned()),
		next_action: operator_execution_program_node_next_action(node, &reason_codes),
		reason_codes,
		reasons,
	}
}

pub(crate) fn operator_execution_program_missing_contract_nodes(
	record: &ExecutionProgramRecord,
) -> Vec<OperatorExecutionProgramNodeStatus> {
	record
		.program()
		.nodes()
		.iter()
		.map(|node| {
			let issue = node.linear_issue();

			OperatorExecutionProgramNodeStatus {
				program_stage: node.stage().as_str().to_owned(),
				lifecycle_state: String::from("stale"),
				readiness_state: String::from("stale"),
				issue_identifier: issue.map(|issue| issue.issue_identifier().to_owned()),
				issue_state: issue.map(|issue| issue.issue_state().to_owned()),
				dispatch_action: None,
				reason_codes: vec![String::from("source_decision_contract_missing")],
				reasons: vec![String::from("source Decision Contract is missing")],
				next_action: String::from(
					"Restore or supersede the source Decision Contract before dispatching this program.",
				),
			}
		})
		.collect()
}

pub(crate) fn operator_execution_program_mapped_issue_identifiers(
	record: &ExecutionProgramRecord,
) -> Vec<String> {
	let mut identifiers = record
		.program()
		.nodes()
		.iter()
		.filter_map(|node| node.linear_issue().map(|issue| issue.issue_identifier().to_owned()))
		.collect::<Vec<_>>();

	identifiers.sort();
	identifiers.dedup();

	identifiers
}

pub(crate) fn operator_execution_program_reason_codes(reasons: &[String]) -> Vec<String> {
	let mut seen = BTreeSet::new();

	for reason in reasons {
		seen.insert(operator_execution_program_reason_code(reason).to_owned());
	}

	seen.into_iter().collect()
}

pub(crate) fn operator_execution_program_reason_code(reason: &str) -> &'static str {
	if reason == "node no longer matches the accepted Decision Contract" {
		"accepted_contract_mismatch"
	} else if reason == "node dispatch intent is not-ready" {
		"dispatch_intent_not_ready"
	} else if reason == "node dispatch intent is paused" {
		"dispatch_intent_paused"
	} else if reason == "node already has a current lane" {
		"current_lane_present"
	} else if reason == "node dispatch intent is terminal" {
		"dispatch_intent_terminal"
	} else if reason == "node is ready for normal Linear issue execution" {
		"ready_for_linear_execution"
	} else if reason == "node has no acceptance expectations" {
		"acceptance_expectations_missing"
	} else if reason == "node has no validation expectations" {
		"validation_expectations_missing"
	} else if reason.starts_with("dependency `") {
		"dependency_not_terminal"
	} else if reason.starts_with("conflict domain `") {
		"conflict_domain_occupied"
	} else if reason == "node has no normal Linear issue mapping" {
		"linear_issue_mapping_missing"
	} else if reason.contains(" is already terminal in `") {
		"mapped_issue_terminal"
	} else if reason.contains(" is not in a startable state") {
		"mapped_issue_not_startable"
	} else if reason.contains(" already carries `") {
		"mapped_issue_active_label_present"
	} else if reason.contains(" is owned by the retained post-review lifecycle") {
		"mapped_issue_post_review_owner"
	} else if reason.contains(" carries `decodex:manual-only`") {
		"mapped_issue_manual_only"
	} else if reason.contains(" carries `decodex:needs-attention`") {
		"mapped_issue_needs_attention"
	} else if reason.contains(" has open tracker dependency blockers") {
		"mapped_issue_open_blockers"
	} else if reason.contains(" is missing a generic dispatch briefing") {
		"mapped_issue_dispatch_briefing_missing"
	} else {
		"program_readiness_blocked"
	}
}

pub(crate) fn operator_execution_program_public_reason(reason: &str) -> String {
	if reason.starts_with("conflict domain `") {
		String::from("another active or retained program node occupies this conflict domain")
	} else if reason.contains(" is owned by the retained post-review lifecycle") {
		String::from(
			"Review & Landing owns this issue until post-review landing or closeout finishes",
		)
	} else if reason.starts_with("dependency `") {
		String::from("a dependency has not reached a required terminal state")
	} else {
		reason.to_owned()
	}
}

pub(crate) fn operator_execution_program_node_next_action(
	node: &ExecutionNodeEvaluation,
	reason_codes: &[String],
) -> String {
	if matches!(
		node.lifecycle_state(),
		crate::execution_program::ExecutionProgramNodeLifecycleState::Stale
			| crate::execution_program::ExecutionProgramNodeLifecycleState::Superseded
	) {
		return String::from(
			"Refresh or supersede the accepted Decision Contract before dispatching this program.",
		);
	}
	if reason_codes.iter().any(|code| code == "dependency_not_terminal") {
		return String::from(
			"Complete the dependency issue or refresh the Execution Program dependency plan if this remains stale.",
		);
	}
	if matches!(
		node.lifecycle_state(),
		crate::execution_program::ExecutionProgramNodeLifecycleState::NeedsAttention
	) || reason_codes.iter().any(|code| code == "mapped_issue_needs_attention")
	{
		return String::from(
			"Resolve the mapped issue's needs-attention stop before dispatching this node.",
		);
	}
	if matches!(
		node.lifecycle_state(),
		crate::execution_program::ExecutionProgramNodeLifecycleState::Active
	) {
		return String::from(
			"Wait for the current lane or recover its retained state before dispatching this node.",
		);
	}
	if matches!(
		node.lifecycle_state(),
		crate::execution_program::ExecutionProgramNodeLifecycleState::PostReview
	) || reason_codes.iter().any(|code| code == "mapped_issue_post_review_owner")
	{
		return String::from(
			"Continue the retained post-review lifecycle before dispatching this program node.",
		);
	}
	if node.dispatch_action().is_some() {
		return String::from("The program scheduler can dispatch this node directly.");
	}
	if matches!(
		node.lifecycle_state(),
		crate::execution_program::ExecutionProgramNodeLifecycleState::Planned
			| crate::execution_program::ExecutionProgramNodeLifecycleState::Mapped
	) {
		return String::from("Map, promote, or unpause this intake node before dispatching it.");
	}
	if matches!(
		node.lifecycle_state(),
		crate::execution_program::ExecutionProgramNodeLifecycleState::Blocked
	) {
		return String::from(
			"Repair mapped issue blockers, briefing, or program readiness before retrying.",
		);
	}

	String::from("No operator action required.")
}
