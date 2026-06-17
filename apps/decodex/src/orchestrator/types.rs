use state::PrivateExecutionEvent;

use crate::tracker;

type PullRequestReadbackResult =
	std::result::Result<PullRequestReviewState, PullRequestReadbackFailure>;

pub(crate) const AUTHORITY_DECISION_REQUEST_SCHEMA: &str =
	"decodex.authority_decision_request/1";
pub(crate) const AUTHORITY_DECISION_REQUEST_EVENT_TYPE: &str = "authority_decision_request";
#[allow(dead_code)]
pub(crate) const AUTHORITY_BOUNDARY_CHECK_EVENT_TYPE: &str = "authority_boundary_check";
pub(crate) const ARCHITECTURE_RECOVERY_PACKET_EVENT_TYPE: &str =
	"architecture_recovery_packet";
pub(crate) const ARCHITECTURE_RECOVERY_STARTED_EVENT_TYPE: &str =
	"architecture_recovery_started";
pub(crate) const ARCHITECTURE_RECOVERY_TERMINAL_EVENT_TYPE: &str =
	"architecture_recovery_terminal";
pub(crate) const PHASE_GOAL_RECOVERY_EVENT_TYPE: &str = "phase_goal_recovery";
pub(crate) const PHASE_GOAL_RECOVERY_BLOCKED_EVENT_TYPE: &str =
	"phase_goal_recovery_blocked";
pub(crate) const PHASE_GOAL_RECOVERY_AUTOMATIC_CONTINUATION_LIMIT: i64 = 1;

#[allow(dead_code)]
const AUTHORITY_BOUNDARY_CHECK_SCHEMA: &str = "decodex.authority_boundary_check/1";
const ARCHITECTURE_RECOVERY_PACKET_SCHEMA: &str = "decodex.architecture_recovery_packet/1";

trait PullRequestReviewStateInspector {
	fn inspect_review_state(
		&self,
		cwd: &Path,
		pr_url: &str,
	) -> crate::prelude::Result<PullRequestReviewState>;

	fn inspect_review_state_readback(
		&self,
		cwd: &Path,
		pr_url: &str,
	) -> PullRequestReadbackResult {
		self.inspect_review_state(cwd, pr_url)
			.map_err(PullRequestReadbackFailure::from)
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum IssueDispatchMode {
	Normal,
	Program,
	Retry,
	ReviewRepair,
	Closeout,
}
impl IssueDispatchMode {
	fn as_str(self) -> &'static str {
		match self {
			Self::Normal => "normal",
			Self::Program => "program",
			Self::Retry => "retry",
			Self::ReviewRepair => "review_repair",
			Self::Closeout => "closeout",
		}
	}

	fn allows_issue(
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

				issue_passes_dispatch_policy(tracker, issue, workflow, &queue_label, false)
			},
			Self::Program => {
				let queue_label = tracker::automation_queue_label(project.service_id());

				issue_passes_dispatch_policy(tracker, issue, workflow, &queue_label, true)
			},
			Self::Retry => issue_passes_retry_dispatch_policy(
				tracker,
				issue,
				project,
				workflow,
				state_store,
				hint,
			),
			Self::ReviewRepair => {
				Ok(issue_passes_review_repair_dispatch_policy(tracker, issue, project, workflow)?
					&& !issue_retry_budget_exhausted(workflow, state_store, &issue.id)?)
			},
			Self::Closeout => {
				issue_passes_closeout_dispatch_policy(tracker, issue, project, workflow, state_store)
			},
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
	fn as_str(self) -> &'static str {
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

/// Final authority disposition for one loop recovery boundary check.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AuthorityBoundaryDisposition {
	WithinAuthority,
	RequiresHuman,
	InsufficientEvidence,
}
impl AuthorityBoundaryDisposition {
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::WithinAuthority => "within_authority",
			Self::RequiresHuman => "requires_human",
			Self::InsufficientEvidence => "insufficient_evidence",
		}
	}
}

pub(crate) enum RetryDispatchDecision {
	Blocked { excluded_issue_ids: Vec<String> },
	Dispatch(Box<RunSummary>),
	Continue,
}

#[derive(Clone, Debug)]
pub(crate) enum RunLeaseDisposition {
	RetainedReviewComplete,
	Superseded {
		newer_run_id: String,
		newer_attempt_number: i64,
	},
	Terminal,
	NotDispatchable,
	Stalled { idle_for: Duration },
	StalledRetainedPartialProgress { idle_for: Duration },
	StalledAlreadyNeedsAttention { idle_for: Duration },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PullRequestReadbackRootCause {
	MissingGithubCli,
	MissingGithubToken,
	GithubAuthFailed,
	GithubApiReadFailed,
	GithubResponseParseFailed,
	PullRequestShapeReadFailed,
	LineageValidationFailed,
	TrackerIssueReadbackFailed,
}
impl PullRequestReadbackRootCause {
	fn as_str(self) -> &'static str {
		match self {
			Self::MissingGithubCli => "missing_github_cli",
			Self::MissingGithubToken => "missing_github_token",
			Self::GithubAuthFailed => "github_auth_failed",
			Self::GithubApiReadFailed => "github_api_read_failed",
			Self::GithubResponseParseFailed => "github_response_parse_failed",
			Self::PullRequestShapeReadFailed => "pull_request_shape_read_failed",
			Self::LineageValidationFailed => "lineage_validation_failed",
			Self::TrackerIssueReadbackFailed => "tracker_issue_readback_failed",
		}
	}
}

enum RetainedReviewLaneLoad {
	Skip,
	Wait(String),
	Ready(Box<RetainedReviewLane>),
	Blocked(Box<RetainedReviewLaneBlocked>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReviewOrchestrationPhase {
	RequestPending,
	WaitingForAck,
	WaitingForResult,
	RepairRequired,
	PassWaitingForGates,
	WaitingForMerge,
}
impl ReviewOrchestrationPhase {
	fn as_str(self) -> &'static str {
		match self {
			Self::RequestPending => "request_pending",
			Self::WaitingForAck => "waiting_for_ack",
			Self::WaitingForResult => "waiting_for_result",
			Self::RepairRequired => "repair_required",
			Self::PassWaitingForGates => "pass_waiting_for_gates",
			Self::WaitingForMerge => "waiting_for_merge",
		}
	}

	fn parse(value: &str) -> std::result::Result<Self, String> {
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

enum PostReviewLaneStateLoad {
	Classification(PostReviewLaneClassification),
	ReviewState(PullRequestReviewState),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LoopGuardrailReason {
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
	fn error_class(self) -> &'static str {
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

	fn from_error_class(error_class: &str) -> Option<Self> {
		match error_class {
			"validation_repeat" | "validation_failure_repeated" => Some(Self::ValidationRepeat),
			"no_effective_diff" => Some(Self::NoEffectiveDiff),
			"remaining_delta_unchanged" => Some(Self::RemainingDeltaUnchanged),
			"review_churn" | "review_policy_exhausted" => Some(Self::ReviewChurn),
			"review_handoff_state_drift" | "review_handoff_rebind_required" => {
				Some(Self::ReviewHandoffStateDrift)
			},
			"dependency_program_stale" | "dependency_blocked" => {
				Some(Self::DependencyProgramStale)
			},
			"uncovered_direction" | "research_contract_required" => {
				Some(Self::UncoveredDirection)
			},
			"ambiguous_retained_progress" | "ownership_ambiguous" => {
				Some(Self::AmbiguousRetainedProgress)
			},
			_ => None,
		}
	}

	fn terminal_next_action(self, recovery_gate: &str) -> String {
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

/// One surface considered by an authority boundary check.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AuthorityBoundaryChangedSurface<'a> {
	pub(crate) surface: &'a str,
	pub(crate) change_summary: &'a str,
	pub(crate) classification: AuthorityBoundaryDisposition,
}

/// Sanitized harness feedback emitted from an authority boundary check.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AuthorityBoundaryImprovementSignal<'a> {
	pub(crate) kind: &'a str,
	pub(crate) reason_code: &'a str,
	pub(crate) target: &'a str,
	pub(crate) recommendation: &'a str,
}

/// Input for persisting a structured authority boundary check as private evidence.
#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AuthorityBoundaryCheckInput<'a> {
	pub(crate) project_id: &'a str,
	pub(crate) issue_id: &'a str,
	pub(crate) issue_identifier: &'a str,
	pub(crate) run_id: &'a str,
	pub(crate) attempt_number: i64,
	pub(crate) decision_contract_ids: Vec<&'a str>,
	pub(crate) attempted_recovery_reason: &'a str,
	pub(crate) changed_surfaces: Vec<AuthorityBoundaryChangedSurface<'a>>,
	pub(crate) disposition: AuthorityBoundaryDisposition,
	pub(crate) final_disposition_reason: &'a str,
	pub(crate) improvement_signals: Vec<AuthorityBoundaryImprovementSignal<'a>>,
}

/// One public-safe option offered in a durable authority decision request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AuthorityDecisionOption<'a> {
	pub(crate) label: &'a str,
	pub(crate) description: &'a str,
}

/// Input for persisting the full local decision packet for an authority-boundary stop.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AuthorityDecisionRequestInput<'a> {
	pub(crate) project_id: &'a str,
	pub(crate) issue_id: &'a str,
	pub(crate) issue_identifier: &'a str,
	pub(crate) run_id: &'a str,
	pub(crate) attempt_number: i64,
	pub(crate) boundary_check_record_id: i64,
	pub(crate) decision_request_id: &'a str,
	pub(crate) reason_code: &'a str,
	pub(crate) boundary_type: &'a str,
	pub(crate) proposed_change: &'a str,
	pub(crate) why_exceeds_authority: &'a str,
	pub(crate) options: Vec<AuthorityDecisionOption<'a>>,
	pub(crate) recommendation: &'a str,
	pub(crate) resume_condition: &'a str,
	pub(crate) retained_worktree_evidence: Vec<&'a str>,
	pub(crate) retained_diff_evidence: Vec<&'a str>,
	pub(crate) recovery_attempt_context: Vec<&'a str>,
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
	project_id: String,
	issue_id: String,
	issue_identifier: String,
	issue_state: String,
	initial_issue_state: String,
	#[cfg(test)]
	retry_project_slug: String,
	dispatch_mode: IssueDispatchMode,
	branch_name: String,
	worktree_path: PathBuf,
	attempt_number: i64,
	run_id: String,
	continuation_pending: bool,
}

#[derive(Debug)]
struct PullRequestReadbackFailure {
	root_cause: PullRequestReadbackRootCause,
	error: Report,
}
impl PullRequestReadbackFailure {
	fn from_report(error: Report) -> Self {
		let root_cause = classify_pull_request_readback_report(&error);

		Self { root_cause, error }
	}

	fn into_report(self) -> Report {
		self.error
	}

	fn root_cause(&self) -> PullRequestReadbackRootCause {
		self.root_cause
	}
}

impl From<Report> for PullRequestReadbackFailure {
	fn from(error: Report) -> Self {
		Self::from_report(error)
	}
}

struct MaterializedDaemonSpawnState {
	worktree: WorktreeSpec,
	retry_budget_base: i64,
}

#[derive(Clone, Debug)]
struct IssueRunPlan {
	issue: TrackerIssue,
	issue_state: String,
	initial_issue_state: String,
	worktree: WorktreeSpec,
	#[allow(dead_code)]
	#[cfg(test)]
	retry_project_slug: String,
	dispatch_mode: IssueDispatchMode,
	attempt_number: i64,
	run_id: String,
	retry_budget_base: i64,
}

#[derive(Default)]
struct RecoveredRuntimeState {
	recoverable_issues: Vec<TrackerIssue>,
}

#[derive(Clone, Copy)]
struct RunCycleRequest<'a> {
	config_path: &'a Path,
	state_store: &'a StateStore,
	dry_run: bool,
	preferred_issue_id: Option<&'a str>,
	preferred_issue_state: Option<&'a str>,
	preferred_initial_issue_state: Option<&'a str>,
	preferred_lease_acquired: bool,
	preferred_issue_claim_fd: Option<i32>,
	preferred_dispatch_slot_fd: Option<i32>,
	preferred_dispatch_slot_index: Option<usize>,
	preferred_dispatch_mode: Option<IssueDispatchMode>,
	preferred_run_identity: Option<PreferredRunIdentity<'a>>,
	preferred_retry_budget_base: Option<i64>,
	preferred_workflow_snapshot: Option<&'a str>,
}

struct SpawnRunOnceChildRequest<'a> {
	config_path: &'a Path,
	preferred_issue_id: &'a str,
	preferred_issue_state: &'a str,
	preferred_initial_issue_state: Option<&'a str>,
	dispatch_mode: IssueDispatchMode,
	preferred_run_id: &'a str,
	preferred_attempt_number: i64,
	preferred_retry_budget_base: i64,
	workflow: &'a WorkflowDocument,
	issue_claim_handoff: Option<&'a File>,
	dispatch_slot_handoff: Option<&'a File>,
	dispatch_slot_index_handoff: Option<usize>,
}

#[derive(Clone, Copy)]
struct PrepareIssueRunContext<'a, T> {
	tracker: &'a T,
	project: &'a ServiceConfig,
	workflow: &'a WorkflowDocument,
	state_store: &'a StateStore,
	worktree_manager: &'a WorktreeManager,
	dry_run: bool,
	lease_preacquired: bool,
	dispatch_mode: IssueDispatchMode,
	preferred_issue_state: Option<&'a str>,
	preferred_initial_issue_state: Option<&'a str>,
	preferred_run_identity: Option<PreferredRunIdentity<'a>>,
	preferred_retry_budget_base: Option<i64>,
}

struct IssueTurnContinuationGuard<'a, T> {
	tracker: &'a T,
	tracker_tool_bridge: &'a TrackerToolBridge<'a>,
	workflow: &'a WorkflowDocument,
	service_id: &'a str,
	issue_id: &'a str,
	issue_identifier: &'a str,
	initial_issue_state: &'a str,
	#[allow(dead_code)]
	#[cfg(test)]
	retry_project_slug: &'a str,
	dispatch_mode: IssueDispatchMode,
	review_state_inspector: Option<&'a dyn PullRequestReviewStateInspector>,
}
impl<T> IssueTurnContinuationGuard<'_, T>
where
	T: IssueTracker,
{
	fn issue_has_service_ownership(&self, issue: &TrackerIssue) -> crate::prelude::Result<bool> {
		tracker::issue_has_label_with_server_confirmation(
			self.tracker,
			issue,
			&tracker::automation_active_label(self.service_id),
		)
	}

	fn completed_closeout_pr_is_merged(&self) -> crate::prelude::Result<bool> {
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
			let issue_completed_with_merged_pr = issue.state.name == completed_state
				&& self.completed_closeout_pr_is_merged()?;

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
struct ManualAttentionRequested {
	issue_identifier: String,
	label: String,
	run_id: String,
	error_class: Option<String>,
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
struct ReviewHandoffNeedsAttention {
	issue_identifier: String,
	pr_url: String,
	run_id: String,
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
struct RetainedReviewNeedsAttention {
	reason: String,
}
impl Display for RetainedReviewNeedsAttention {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		write!(
			f,
			"Retained review orchestration requires operator attention: {}.",
			self.reason
		)
	}
}

impl Error for RetainedReviewNeedsAttention {}

#[derive(Debug)]
struct RetainedPartialProgress {
	issue_identifier: String,
	run_id: String,
	worktree_path: String,
	source_error_class: Option<String>,
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
struct LoopGuardrailStopRequested {
	issue_identifier: String,
	run_id: String,
	reason: LoopGuardrailReason,
	consecutive_count: i64,
	fingerprint: String,
	source_error_class: Option<String>,
	architecture_recovery_reason_code: Option<String>,
}
impl LoopGuardrailStopRequested {
	fn terminal_error_class(&self) -> &'static str {
		match self.architecture_recovery_reason_code.as_deref() {
			Some("architecture_recovery_exhausted") => "architecture_recovery_exhausted",
			Some("contract_boundary_required") => "contract_boundary_required",
			Some("external_dependency_required") => "external_dependency_required",
			Some("architecture_recovery_started") | None => self.reason.error_class(),
			Some(_) => self.reason.error_class(),
		}
	}

	fn terminal_next_action(&self, recovery_gate: &str) -> String {
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
		let architecture_recovery = self
			.architecture_recovery_reason_code
			.as_deref()
			.unwrap_or("none");

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
struct AgentGitCredentialsUnavailable {
	run_id: String,
	token_env_var: String,
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
struct StalledRunNeedsAttention {
	issue_identifier: String,
	run_id: String,
	idle_for: Duration,
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

struct DaemonRunChild {
	child: Child,
	issue_id: String,
	run_id: String,
	attempt_number: i64,
	initial_issue_state: String,
	#[allow(dead_code)]
	#[cfg(test)]
	retry_project_slug: String,
	dispatch_mode: IssueDispatchMode,
	from_retry_queue: bool,
	workflow: WorkflowDocument,
}

#[derive(Clone, Copy)]
struct ChildRunRef<'a> {
	issue_id: &'a str,
	run_id: &'a str,
	attempt_number: i64,
}

#[derive(Clone, Copy)]
struct CurrentChildRunContext<'a> {
	child: ChildRunRef<'a>,
	workflow: &'a WorkflowDocument,
	dispatch_mode: IssueDispatchMode,
}

#[derive(Clone, Copy)]
struct PreferredRunIdentity<'a> {
	run_id: &'a str,
	attempt_number: i64,
}

#[derive(Clone, Debug)]
struct RetryEntry {
	issue_id: String,
	#[allow(dead_code)]
	#[cfg(test)]
	retry_project_slug: String,
	continuation_initial_issue_state: Option<String>,
	dispatch_mode: IssueDispatchMode,
	kind: RetryKind,
	attempt: u32,
	ready_at: Instant,
}

#[derive(Default)]
struct RetryQueue {
	entries: HashMap<String, RetryEntry>,
}
impl RetryQueue {
	fn is_empty(&self) -> bool {
		self.entries.is_empty()
	}

	fn upsert(&mut self, entry: RetryEntry) {
		self.entries.insert(entry.issue_id.clone(), entry);
	}

	fn release(&mut self, issue_id: &str) {
		self.entries.remove(issue_id);
	}

	fn next_entry(&self) -> Option<&RetryEntry> {
		self.entries.values().min_by(|left, right| {
			left.ready_at.cmp(&right.ready_at).then_with(|| left.issue_id.cmp(&right.issue_id))
		})
	}

	fn ordered_entries(&self) -> Vec<RetryEntry> {
		let mut entries = self.entries.values().cloned().collect::<Vec<_>>();

		entries.sort_by(|left, right| {
			left.ready_at.cmp(&right.ready_at).then_with(|| left.issue_id.cmp(&right.issue_id))
		});

		entries
	}
}

#[derive(Default)]
struct RecoverableWorktreeSkipCache {
	entries: HashMap<String, Instant>,
}
impl RecoverableWorktreeSkipCache {
	fn is_suppressed(&mut self, issue_identifier: &str, now: Instant) -> bool {
		self.retain_active(now);

		self.entries.get(&issue_identifier.to_ascii_uppercase()).is_some_and(|until| *until > now)
	}

	fn remember(&mut self, issue_identifier: &str, now: Instant) {
		self.retain_active(now);
		self.entries.insert(
			issue_identifier.to_ascii_uppercase(),
			now + RECOVERABLE_WORKTREE_SKIP_TTL,
		);
	}

	fn retain_active(&mut self, now: Instant) {
		self.entries.retain(|_, until| *until > now);
	}
}

struct DaemonTickContext {
	config: ServiceConfig,
	workflow: WorkflowDocument,
	tracker: LinearClient,
	worktree_manager: WorktreeManager,
}

#[derive(Default)]
struct ProjectDaemonRuntime {
	active_children: Vec<DaemonRunChild>,
	retry_queue: RetryQueue,
	tracker_backoff: Option<TrackerConnectorBackoff>,
	next_linear_scan_at: Option<Instant>,
	workflow_cache: Option<CachedWorkflowDocument>,
	recoverable_worktree_skip_cache: RecoverableWorktreeSkipCache,
}

	#[derive(Clone, Debug)]
	struct TrackerConnectorBackoff {
	until: Instant,
	reset_unix_epoch: i64,
	reset_source: &'static str,
	sync_phase: &'static str,
}

struct OperatorStateEndpoint {
	listen_address: SocketAddr,
	snapshot: Arc<Mutex<PublishedOperatorSnapshot>>,
	dashboard_events: DashboardEventHub,
	control_requests: OperatorControlRequests,
	shutdown_tx: Sender<()>,
	activity_shutdown_tx: Sender<()>,
	server_thread: Option<JoinHandle<()>>,
	activity_thread: Option<JoinHandle<()>>,
}
impl OperatorStateEndpoint {
	fn start(listen_address: &str, state_store: Arc<StateStore>) -> crate::prelude::Result<Self> {
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

	fn listen_address(&self) -> SocketAddr {
		self.listen_address
	}

	fn publish_snapshot(&self, snapshot: &OperatorStatusSnapshot) -> crate::prelude::Result<()> {
		let snapshot_json = serde_json::to_vec(snapshot)?;
		let snapshot_value = serde_json::to_value(snapshot)?;
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

	fn drain_linear_scan_requests(&self) -> crate::prelude::Result<Vec<OperatorLinearScanRequest>> {
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
struct PublishedOperatorSnapshot {
	snapshot_json: Option<Vec<u8>>,
	last_publish_unix_epoch: Option<i64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct OperatorLinearScanRequest {
	project_id: Option<String>,
}

#[derive(Clone, Default)]
struct OperatorControlRequests {
	linear_scan_requests: Arc<Mutex<Vec<OperatorLinearScanRequest>>>,
}
impl OperatorControlRequests {
	fn request_linear_scan(&self, project_id: Option<String>) -> crate::prelude::Result<()> {
		let mut requests = self
			.linear_scan_requests
			.lock()
			.map_err(|error| eyre::eyre!("Operator control request lock poisoned: {error}"))?;

		requests.push(OperatorLinearScanRequest { project_id });

		Ok(())
	}

	fn drain_linear_scan_requests(&self) -> crate::prelude::Result<Vec<OperatorLinearScanRequest>> {
		let mut requests = self
			.linear_scan_requests
			.lock()
			.map_err(|error| eyre::eyre!("Operator control request lock poisoned: {error}"))?;

		Ok(requests.drain(..).collect())
	}
}

#[derive(Clone)]
struct CachedWorkflowDocument {
	path: PathBuf,
	document: WorkflowDocument,
}

#[derive(Clone, Copy)]
struct ActiveWorkflowOverride<'a> {
	child: ChildRunRef<'a>,
	workflow: &'a WorkflowDocument,
}

#[derive(Clone, Debug)]
struct RunLeaseReconciliation {
	issue: TrackerIssue,
	run_attempt: RunAttempt,
	worktree_mapping: Option<WorktreeMapping>,
	disposition: RunLeaseDisposition,
	workflow: WorkflowDocument,
}

struct TerminalFailureOutcome {
	error_class: &'static str,
	retry_guarded_by_state: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct OperatorStatusSnapshot {
	project_id: String,
	run_limit: usize,
	#[serde(skip_serializing_if = "Option::is_none")]
	status_source: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	snapshot_age_seconds: Option<i64>,
	warnings: Vec<String>,
	warning_details: Vec<OperatorSnapshotWarningDetail>,
	connector_backoffs: Vec<OperatorConnectorBackoffStatus>,
	projects: Vec<OperatorProjectStatus>,
	account_control: OperatorCodexAccountControlStatus,
	accounts: Vec<CodexAccountActivitySummary>,
	current_lanes: Vec<OperatorRunStatus>,
	recent_runs: Vec<OperatorRunStatus>,
	history_lanes: Vec<OperatorHistoryLaneStatus>,
	execution_programs: Vec<OperatorExecutionProgramStatus>,
	queued_candidates: Vec<OperatorQueuedIssueStatus>,
	worktrees: Vec<OperatorWorktreeStatus>,
	post_review_lanes: Vec<OperatorPostReviewLaneStatus>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct OperatorSnapshotWarningDetail {
	warning: String,
	project_id: Option<String>,
	repo_root: Option<String>,
	reason: String,
	next_action: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct OperatorConnectorBackoffStatus {
	project_id: String,
	connector: String,
	sync_phase: String,
	quota_class: String,
	reset_at: String,
	reset_unix_epoch: i64,
	reset_source: String,
	retry_after_seconds: i64,
	next_action: String,
	warning: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct OperatorProjectStatus {
	project_id: String,
	config_path: String,
	repo_root: String,
	enabled: bool,
	github_cli_authority: OperatorGitHubCliAuthority,
	current_lane_count: usize,
	running_lane_count: usize,
	queued_candidate_count: usize,
	post_review_lane_count: usize,
	retained_worktree_count: usize,
	waiting_lane_count: usize,
	attention_count: usize,
	cleanup_blocked_count: usize,
	cleanup_pending_count: usize,
	connector_state: String,
	last_activity_at: Option<String>,
	warning_count: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct OperatorExecutionProgramStatus {
	program_id: String,
	#[serde(default = "operator_execution_program_unknown_status")]
	status: String,
	source_contract_id: Option<String>,

	intake_kind: Option<String>,

	public_summary: Option<String>,
	node_count: usize,
	planned_count: usize,
	mapped_count: usize,
	ready_count: usize,
	queued_count: usize,
	blocked_count: usize,
	held_count: usize,
	active_count: usize,
	needs_attention_count: usize,
	completed_count: usize,
	stale_count: usize,
	superseded_count: usize,
	dispatchable_count: usize,
	mapped_issue_identifiers: Vec<String>,
	#[serde(default)]
	node_readbacks: Vec<OperatorExecutionProgramNodeStatus>,
	readback_warning: Option<String>,
}
impl OperatorExecutionProgramStatus {
	fn from_summary(
		record: &ExecutionProgramRecord,
		summary: ExecutionProgramOperatorSummary,
		evaluation: &ExecutionProgramEvaluation,
	) -> Self {
		let program_intake_plan = record.program().program_intake_plan();

		Self {
			status: operator_execution_program_status(&summary, record.program().nodes().len(), None),
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

	fn missing_contract(record: &ExecutionProgramRecord) -> Self {
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
struct OperatorExecutionProgramNodeStatus {
	#[serde(default = "operator_execution_program_unknown_status")]
	program_stage: String,
	lifecycle_state: String,
	readiness_state: String,
	issue_identifier: Option<String>,
	issue_state: Option<String>,
	dispatch_action: Option<String>,
	reason_codes: Vec<String>,
	reasons: Vec<String>,
	next_action: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct OperatorGitHubCliAuthority {
	command_path: String,
	resolved_path: Option<String>,
	configured_path: Option<String>,
	discovery_tier: String,
	available: bool,
	next_action: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct OperatorCodexAccountControlStatus {
	mode: String,
	account_selector: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct OperatorHistoryLaneStatus {
	project_id: String,
	issue_id: String,
	issue_identifier: Option<String>,
	title: Option<String>,
	author: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	issue_state: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	active_label_present: Option<bool>,
	#[serde(skip_serializing_if = "Option::is_none")]
	needs_attention_label_present: Option<bool>,
	issue_key: String,
	attempt_count: usize,
	ledger_outcome: OperatorHistoryLedgerOutcome,
	lifecycle_metrics: OperatorLaneLifecycleMetrics,
	latest_run: OperatorRunStatus,
	attempts: Vec<OperatorRunStatus>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
struct OperatorLaneLifecycleMetrics {
	attempt_count: usize,
	run_count: usize,
	recorded_attempt_count: usize,
	recovered_attempt_count: usize,
	current_snapshot_attempt_count: usize,
	captured_attempt_count: usize,
	missing_attempt_count: usize,
	protocol_event_count: i64,
	child_event_count: i64,
	wall_seconds: i64,
	tool_call_count: i64,
	input_tokens_current: Option<i64>,
	input_tokens_peak: Option<i64>,
	input_tokens_cumulative: i64,
	output_tokens_cumulative: i64,
	largest_tool_output_bytes: Option<i64>,
	largest_tool_output_tool: Option<String>,
	large_output_warnings: Vec<String>,
	buckets: Vec<ChildAgentActivityBucket>,
	phases: Vec<OperatorLaneLifecyclePhaseMetrics>,
	attempt_evidence: Vec<OperatorLaneLifecycleAttemptEvidence>,
	recovery_gaps: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
struct OperatorLaneLifecyclePhaseMetrics {
	phase: String,
	label: String,
	attempt_count: usize,
	run_count: usize,
	recorded_attempt_count: usize,
	recovered_attempt_count: usize,
	current_snapshot_attempt_count: usize,
	captured_attempt_count: usize,
	missing_attempt_count: usize,
	protocol_event_count: i64,
	child_event_count: i64,
	wall_seconds: i64,
	tool_call_count: i64,
	input_tokens_current: Option<i64>,
	input_tokens_peak: Option<i64>,
	input_tokens_cumulative: i64,
	output_tokens_cumulative: i64,
	largest_tool_output_bytes: Option<i64>,
	largest_tool_output_tool: Option<String>,
	large_output_warnings: Vec<String>,
	buckets: Vec<ChildAgentActivityBucket>,
	attempt_evidence: Vec<OperatorLaneLifecycleAttemptEvidence>,
	recovery_gaps: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
struct OperatorLaneLifecycleAttemptEvidence {
	run_id: String,
	issue_id: String,
	attempt_number: i64,
	status: String,
	phase: String,
	source: String,
	evidence: Vec<String>,
	gaps: Vec<String>,
	protocol_event_count: i64,
	child_event_count: i64,
	updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct OperatorHistoryLedgerOutcome {
	ledger_status: String,
	final_outcome: String,
	final_event_type: Option<String>,
	final_event_at: Option<String>,
	summary: Option<String>,
	pr_url: Option<String>,
	commit_sha: Option<String>,
	branch: Option<String>,
	closeout_status: Option<String>,
	needs_attention_reason: Option<String>,
	lifecycle_started_at: Option<String>,
	lifecycle_finished_at: Option<String>,
	lifecycle_elapsed_seconds: Option<i64>,
	record_count: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct OperatorRunStatus {
	project_id: String,
	project_display_name: String,
	run_id: String,
	issue_id: String,
	issue_identifier: Option<String>,
	title: Option<String>,
	author: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	issue_state: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	active_label_present: Option<bool>,
	#[serde(skip_serializing_if = "Option::is_none")]
	needs_attention_label_present: Option<bool>,
	attempt_number: i64,
	status: String,
	attempt_status: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	status_projection_reason: Option<String>,
	ownership_state: String,
	liveness_state: String,
	policy_state: String,
	terminalization_state: String,
	lane_control_next_action: String,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	lane_control_conditions: Vec<String>,
	phase: String,
	#[serde(default)]
	run_phase: String,
	wait_reason: Option<String>,
	current_operation: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	active_goal_phase: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	public_progress_phase: Option<String>,
	thread_id: Option<String>,
	turn_id: Option<String>,
	thread_status: Option<String>,
	thread_active_flags: Vec<String>,
	interactive_requested: bool,
	continuation_pending: bool,
	#[serde(skip_serializing_if = "Option::is_none")]
	continuation_recovery: Option<OperatorContinuationRecoveryStatus>,
	run_lease: bool,
	queue_lease_state: String,
	execution_liveness: String,
	has_fresh_execution: bool,
	counts_as_running: bool,
	needs_attention: bool,
	updated_at: String,
	last_run_activity_at: Option<String>,
	last_protocol_activity_at: Option<String>,
	last_progress_at: Option<String>,
	idle_for_seconds: Option<i64>,
	protocol_idle_for_seconds: Option<i64>,
	suspected_stall: bool,
	#[serde(skip_serializing_if = "Option::is_none")]
	progress_diagnostic: Option<String>,
	last_event_type: Option<String>,
	last_event_at: Option<String>,
	event_count: i64,
	private_evidence: AgentPrivateEvidenceRef,
	#[serde(skip_serializing_if = "Option::is_none")]
	loop_status: Option<OperatorLoopStatus>,
	control_capability: Option<OperatorRunControlCapability>,
	process_id: Option<u32>,
	process_alive: Option<bool>,
	process_liveness_reason: Option<String>,
	retry_kind: Option<String>,
	next_retry_at: Option<String>,
	effective_model: Option<String>,
	effective_model_provider: Option<String>,
	effective_cwd: Option<String>,
	effective_approval_policy: Option<String>,
	effective_approvals_reviewer: Option<String>,
		effective_sandbox_mode: Option<String>,
		child_agent_activity: Option<ChildAgentActivitySummary>,
		protocol_activity: Option<ProtocolActivitySummary>,
		lifecycle_source: String,
		lifecycle_evidence: Vec<String>,
		lifecycle_gaps: Vec<String>,
		#[serde(default)]
		lifecycle_metrics: OperatorLaneLifecycleMetrics,
		account: Option<CodexAccountActivitySummary>,
		accounts: Vec<CodexAccountActivitySummary>,
		branch_name: Option<String>,
	worktree_path: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct OperatorContinuationRecoveryStatus {
	state: String,
	source_phase: String,
	next_phase: String,
	source_error_class: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	source_error_message: Option<String>,
	recorded_at: String,
	run_id: String,
	attempt_number: i64,
	recovery_count: i64,
	automatic_continuation_limit: i64,
	budget_exceeded: bool,
	next_action: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct OperatorRunControlCapability {
	project_id: String,
	issue_id: String,
	run_id: String,
	attempt_number: i64,
	thread_id: Option<String>,
	turn_id: Option<String>,
	transport: String,
	channel_path: String,
	status: String,
	published_at: String,
	updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct OperatorQueuedIssueStatus {
	project_id: String,
	issue_id: String,
	issue_identifier: String,
	title: String,
	author: Option<String>,
	state: String,
	priority: Option<i64>,
	created_at: String,
	classification: String,
	reason: String,
	attention: Option<OperatorQueuedIssueAttentionStatus>,
	blocker_identifiers: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct OperatorQueuedIssueAttentionStatus {
	summary: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	decision_request: Option<OperatorAuthorityDecisionRequestStatus>,
	run_id: Option<String>,
	attempt_number: Option<i64>,
	current_operation: Option<String>,
	thread_status: Option<String>,
	attempt_status: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	loop_status: Option<OperatorLoopStatus>,
	auto_retry_blocked_reason: Option<String>,
	attention_error_class: Option<String>,
	attention_next_action: Option<String>,
	retry_budget_attempt_count: Option<i64>,
	retry_budget_max_attempts: i64,
	last_activity_at: Option<String>,
	last_progress_at: Option<String>,
	last_event_type: Option<String>,
	event_count: i64,
	process_alive: Option<bool>,
	process_liveness_reason: Option<String>,
	worktree_path: Option<String>,
	worktree_has_tracked_changes: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct OperatorAuthorityDecisionRequestStatus {
	phase: String,
	reason: String,
	boundary: String,
	decision_request_id: String,
	next_action: String,
	recommendation: Option<String>,
	resume_condition: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct OperatorLoopStatus {
	review_level: String,
	autonomy: String,
	summary: String,
	next_action: Option<String>,
	review: Option<OperatorReviewLoopStatus>,
	architecture_recovery: Option<OperatorArchitectureRecoveryStatus>,
	boundary: Option<OperatorBoundaryStatus>,
	decision_request: Option<OperatorAuthorityDecisionRequestStatus>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct OperatorReviewLoopStatus {
	phase: String,
	status: String,
	checkpoint: Option<OperatorReviewCheckpointStatus>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct OperatorReviewCheckpointStatus {
	head_sha: String,
	round: i64,
	nonclean_rounds: i64,
	updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct OperatorArchitectureRecoveryStatus {
	status: String,
	reason_code: String,
	guardrail_reason: Option<String>,
	boundary_disposition: Option<String>,
	round: Option<u64>,
	budget: Option<OperatorRecoveryBudgetStatus>,
	next_action: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct OperatorRecoveryBudgetStatus {
	attempt: u64,
	max_attempts: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct OperatorBoundaryStatus {
	disposition: String,
	reason: Option<String>,
	attempted_recovery_reason: Option<String>,
	changed_surface_count: usize,
	improvement_signal_count: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct OperatorWorktreeStatus {
	project_id: String,
	issue_id: String,
	issue_identifier: Option<String>,
	issue_state: Option<String>,
	branch_name: String,
	worktree_path: String,
	ownership: String,
	ownership_reason: String,
	provenance: OperatorWorktreeProvenanceStatus,
	recovery_next_action: Option<String>,
	hygiene: Option<OperatorWorktreeHygieneStatus>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct OperatorWorktreeProvenanceStatus {
	source: String,
	created_at_unix: Option<i64>,
	updated_at_unix: Option<i64>,
	audit_required: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct OperatorWorktreeHygieneStatus {
	classification: String,
	default_branch: String,
	dirty: bool,
	reason: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct OperatorPostReviewLaneStatus {
	project_id: String,
	issue_id: String,
	issue_identifier: String,
	issue_state: String,
	branch_name: String,
	worktree_path: String,
	classification: String,
	reason: String,
	pr_url: Option<String>,
	pr_head_sha: Option<String>,
	pr_state: Option<String>,
	review_decision: Option<String>,
	mergeable: Option<String>,
	check_state: Option<String>,
	unresolved_review_threads: Option<usize>,
	shadowed_by_current_lane: bool,
	readback_warning: Option<String>,
	readback_root_cause: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	loop_status: Option<OperatorLoopStatus>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PostReviewLaneSnapshot {
	issue: TrackerIssue,
	worktree: WorktreeMapping,
	review_handoff: Option<ReviewHandoffMarker>,
	local_branch_name: Option<String>,
	local_head_oid: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PullRequestReviewState {
	url: String,
	state: String,
	is_draft: bool,
	review_decision: Option<String>,
	merge_commit_allowed: bool,
	pending_review_requests: usize,
	mergeable: String,
	merge_state_status: String,
	head_ref_name: String,
	head_ref_oid: String,
	merge_commit_oid: Option<String>,
	head_repository_name: Option<String>,
	head_repository_owner: Option<String>,
	status_check_rollup_state: Option<String>,
	unresolved_review_threads: usize,
	issue_description_external_review_thumbs_up_count: usize,
	issue_comments: Vec<PullRequestIssueCommentState>,
	reviews: Vec<PullRequestReviewSummaryState>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PostReviewLaneClassification {
	decision: PostReviewLaneDecision,
	reason: String,
	pr_url: Option<String>,
	pr_head_sha: Option<String>,
	pr_state: Option<String>,
	review_decision: Option<String>,
	mergeable: Option<String>,
	check_state: Option<String>,
	unresolved_review_threads: Option<usize>,
	readback_warning: Option<String>,
	readback_root_cause: Option<String>,
}

struct RetainedReviewLaneBlocked {
	issue: TrackerIssue,
	worktree: WorktreeMapping,
	run_identity: RetainedReviewRunIdentity,
	reason: String,
}

struct RetainedReviewRunIdentity {
	run_id: String,
	attempt_number: i64,
}

struct SelectedIssueRunCandidate {
	issue: TrackerIssue,
	dispatch_mode: IssueDispatchMode,
	preferred_run_identity: Option<RetainedReviewRunIdentity>,
}
impl SelectedIssueRunCandidate {
	fn new(issue: TrackerIssue, dispatch_mode: IssueDispatchMode) -> Self {
		Self { issue, dispatch_mode, preferred_run_identity: None }
	}
}

struct GhPullRequestReviewStateInspector {
	github_token_env_var: Option<String>,
	github_command_path: Option<PathBuf>,
}
impl PullRequestReviewStateInspector for GhPullRequestReviewStateInspector {
	fn inspect_review_state(
		&self,
		cwd: &Path,
		pr_url: &str,
	) -> crate::prelude::Result<PullRequestReviewState> {
		self.inspect_review_state_readback(cwd, pr_url)
			.map_err(PullRequestReadbackFailure::into_report)
	}

	fn inspect_review_state_readback(
		&self,
		cwd: &Path,
		pr_url: &str,
	) -> PullRequestReadbackResult {
		let github_token = resolve_configured_env_var(
			"github.token_env_var",
			self.github_token_env_var.as_deref(),
		)?;
		let locator = github::parse_pull_request_url(pr_url)?;
		let mut review_threads_after: Option<String> = None;
		let mut review_state: Option<PullRequestReviewState> = None;
		let mut comments_after: Option<String> = None;

		loop {
			let repository = query_pull_request_review_state_page(PullRequestReviewStatePageQuery {
				cwd,
				owner: &locator.owner,
				repo: &locator.repo,
				number: locator.number,
				review_threads_after: review_threads_after.as_deref(),
				pr_url,
				github_token: github_token.as_str(),
				gh_command_path: self.github_command_path.as_deref(),
			})?;
			let pull_request = repository.pull_request.as_ref().ok_or_else(|| {
				eyre::eyre!("GitHub GraphQL response for `{pr_url}` did not include a pull request.")
			})?;
			let next_cursor = match &mut review_state {
				Some(review_state) =>
					merge_pull_request_review_state_page(review_state, &repository, pull_request)?,
				None => {
					let next_cursor = next_pull_request_review_threads_cursor(pull_request)?;

					comments_after =
						next_pull_request_issue_comments_cursor(&pull_request.comments, pr_url)?;
					review_state = Some(pull_request_review_state_from_page(&repository, pull_request)?);

					next_cursor
				},
			};
			let Some(next_cursor) = next_cursor else {
				break;
			};

			review_threads_after = Some(next_cursor);
		}

		let mut review_state = review_state.ok_or_else(|| {
			eyre::eyre!("GitHub GraphQL response for `{pr_url}` did not include a pull request.")
		})?;

		while let Some(cursor) = comments_after.take() {
			let pull_request = query_pull_request_issue_comments_page(PullRequestIssueCommentsPageQuery {
				cwd,
				owner: &locator.owner,
				repo: &locator.repo,
				number: locator.number,
				comments_after: &cursor,
				pr_url,
				github_token: github_token.as_str(),
				gh_command_path: self.github_command_path.as_deref(),
			})?;

			comments_after = merge_pull_request_issue_comment_page(&mut review_state, &pull_request)?;
		}

		Ok(review_state)
	}
}

#[derive(Clone, Copy, Debug, Default)]
struct RetryIssueStateHint<'a> {
	preferred_issue_state: Option<&'a str>,
	preferred_initial_issue_state: Option<&'a str>,
}

struct ChildExitRetryContext<'a, T> {
	retry_queue: &'a mut RetryQueue,
	tracker: &'a T,
	project: &'a ServiceConfig,
	workflow: &'a WorkflowDocument,
	state_store: &'a StateStore,
}

#[derive(Clone, Copy)]
struct TargetIssueRunContext<'a, T> {
	tracker: &'a T,
	project: &'a ServiceConfig,
	workflow: &'a WorkflowDocument,
	state_store: &'a StateStore,
	issue_id: &'a str,
	preferred_issue_state: Option<&'a str>,
	preferred_initial_issue_state: Option<&'a str>,
	dry_run: bool,
	lease_preacquired: bool,
	preferred_issue_claim_fd: Option<i32>,
	preferred_dispatch_slot_fd: Option<i32>,
	preferred_dispatch_slot_index: Option<usize>,
	dispatch_mode: IssueDispatchMode,
	preferred_run_identity: Option<PreferredRunIdentity<'a>>,
	preferred_retry_budget_base: Option<i64>,
}

struct ConcurrencySnapshot {
	total_leased: usize,
}
impl ConcurrencySnapshot {
	fn new(project_id: &str, state_store: &StateStore) -> crate::prelude::Result<Self> {
		let leases = state_store.list_active_shared_leases(project_id)?;

		Ok(Self { total_leased: leases.len() })
	}

	fn has_global_capacity(&self, execution: &WorkflowExecution) -> bool {
		execution.max_concurrent_agents().has_capacity(self.total_leased)
	}
}

#[derive(Deserialize)]
struct PullRequestReviewStateResponse {
	data: PullRequestReviewStateData,
}

#[derive(Deserialize)]
struct PullRequestReviewStateData {
	repository: Option<PullRequestReviewStateRepository>,
}

#[derive(Deserialize)]
struct PullRequestReviewStateRepository {
	#[serde(rename = "mergeCommitAllowed")]
	merge_commit_allowed: bool,
	#[serde(rename = "pullRequest")]
	pull_request: Option<PullRequestReviewStateNode>,
}

#[derive(Deserialize)]
struct PullRequestIssueCommentsResponse {
	data: PullRequestIssueCommentsData,
}

#[derive(Deserialize)]
struct PullRequestIssueCommentsData {
	repository: Option<PullRequestIssueCommentsRepository>,
}

#[derive(Deserialize)]
struct PullRequestIssueCommentsRepository {
	#[serde(rename = "pullRequest")]
	pull_request: Option<PullRequestIssueCommentsNode>,
}

#[derive(Deserialize)]
struct PullRequestIssueCommentsNode {
	url: String,
	comments: PullRequestIssueCommentConnection,
}

#[derive(Deserialize)]
struct PullRequestReviewStateNode {
	url: String,
	state: String,
	#[serde(rename = "isDraft")]
	is_draft: bool,
	#[serde(rename = "reviewDecision")]
	review_decision: Option<String>,
	#[serde(rename = "reviewRequests")]
	review_requests: PullRequestReviewRequestConnection,
	mergeable: String,
	#[serde(rename = "mergeStateStatus")]
	merge_state_status: String,
	#[serde(rename = "headRefName")]
	head_ref_name: String,
	#[serde(rename = "headRefOid")]
	head_ref_oid: String,
	#[serde(rename = "mergeCommit")]
	merge_commit: Option<PullRequestMergeCommitNode>,
	#[serde(rename = "headRepository")]
	head_repository: Option<PullRequestRepository>,
	#[serde(rename = "headRepositoryOwner")]
	head_repository_owner: Option<PullRequestRepositoryOwner>,
	#[serde(rename = "reactionGroups")]
	reaction_groups: Vec<PullRequestReactionGroup>,
	comments: PullRequestIssueCommentConnection,
	reviews: PullRequestReviewConnection,
	#[serde(rename = "reviewThreads")]
	review_threads: PullRequestReviewThreadConnection,
	commits: PullRequestCommitConnection,
}

#[derive(Deserialize)]
struct PullRequestRepositoryOwner {
	login: String,
}

#[derive(Deserialize)]
struct PullRequestMergeCommitNode {
	oid: String,
}

#[derive(Deserialize)]
struct PullRequestRepository {
	name: String,
}

#[derive(Deserialize)]
struct PullRequestReviewRequestConnection {
	#[serde(rename = "totalCount")]
	total_count: usize,
}

#[derive(Deserialize)]
struct PullRequestReviewThreadConnection {
	nodes: Vec<PullRequestReviewThreadNode>,
	#[serde(rename = "pageInfo")]
	page_info: PullRequestPageInfo,
}

#[derive(Deserialize)]
struct PullRequestReviewThreadNode {
	#[serde(rename = "isResolved")]
	is_resolved: bool,
	#[serde(rename = "isOutdated")]
	is_outdated: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PullRequestIssueCommentState {
	database_id: i64,
	author_login: Option<String>,
	body: String,
	created_at_unix_epoch: i64,
	external_review_eyes_reaction_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PullRequestReviewSummaryState {
	author_login: Option<String>,
	body: String,
	state: String,
	submitted_at_unix_epoch: i64,
}

#[derive(Deserialize)]
struct PullRequestIssueCommentConnection {
	nodes: Vec<PullRequestIssueCommentNode>,
	#[serde(rename = "pageInfo")]
	page_info: PullRequestPageInfo,
}

#[derive(Deserialize)]
struct PullRequestIssueCommentNode {
	#[serde(rename = "databaseId")]
	database_id: i64,
	body: String,
	#[serde(rename = "createdAt")]
	created_at: String,
	author: Option<PullRequestActor>,
	#[serde(rename = "reactionGroups")]
	reaction_groups: Vec<PullRequestReactionGroup>,
}

#[derive(Deserialize)]
struct PullRequestReviewConnection {
	nodes: Vec<PullRequestReviewNode>,
}

#[derive(Deserialize)]
struct PullRequestReviewNode {
	body: String,
	state: String,
	#[serde(rename = "submittedAt")]
	submitted_at: Option<String>,
	author: Option<PullRequestActor>,
}

#[derive(Deserialize)]
struct PullRequestReactionGroup {
	content: String,
	users: PullRequestReactionUsersConnection,
}

#[derive(Deserialize)]
struct PullRequestReactionUsersConnection {
	nodes: Vec<PullRequestActor>,
}

#[derive(Deserialize)]
struct PullRequestActor {
	login: String,
}

#[derive(Deserialize)]
struct PullRequestPageInfo {
	#[serde(rename = "hasNextPage")]
	has_next_page: bool,
	#[serde(rename = "endCursor")]
	end_cursor: Option<String>,
}

#[derive(Deserialize)]
struct PullRequestCommitConnection {
	nodes: Vec<PullRequestCommitNode>,
}

#[derive(Deserialize)]
struct PullRequestCommitNode {
	commit: PullRequestCommitPayload,
}

#[derive(Deserialize)]
struct PullRequestCommitPayload {
	#[serde(rename = "statusCheckRollup")]
	status_check_rollup: Option<PullRequestStatusCheckRollup>,
}

#[derive(Deserialize)]
struct PullRequestStatusCheckRollup {
	state: String,
}

#[allow(dead_code)]
pub(crate) fn record_authority_boundary_check_private_event(
	state_store: &StateStore,
	input: AuthorityBoundaryCheckInput<'_>,
) -> Result<PrivateExecutionEvent> {
	validate_authority_boundary_check_input(&input)?;

	let changed_surfaces = input
		.changed_surfaces
		.iter()
		.map(|surface| {
			json!({
				"surface": surface.surface,
				"change_summary": surface.change_summary,
				"classification": surface.classification.as_str(),
			})
		})
		.collect::<Vec<_>>();
	let improvement_signals = input
		.improvement_signals
		.iter()
		.map(|signal| {
			json!({
				"kind": signal.kind,
				"reason_code": signal.reason_code,
				"target": signal.target,
				"recommendation": signal.recommendation,
			})
		})
		.collect::<Vec<_>>();
	let payload = json!({
		"schema": AUTHORITY_BOUNDARY_CHECK_SCHEMA,
		"record_version": 1,
		"issue": {
			"id": input.issue_id,
			"identifier": input.issue_identifier,
		},
		"run": {
			"run_id": input.run_id,
			"attempt_number": input.attempt_number,
		},
		"decision_contract_ids": input.decision_contract_ids,
		"attempted_recovery_reason": input.attempted_recovery_reason,
		"changed_surfaces": changed_surfaces,
		"disposition": input.disposition.as_str(),
		"final_disposition": {
			"disposition": input.disposition.as_str(),
			"reason": input.final_disposition_reason,
		},
		"improvement_signals": improvement_signals,
	});

	state_store.append_private_execution_event(
		input.project_id,
		input.issue_id,
		input.run_id,
		input.attempt_number,
		AUTHORITY_BOUNDARY_CHECK_EVENT_TYPE,
		payload,
	)
}

pub(crate) fn record_authority_decision_request_private_event(
	state_store: &StateStore,
	input: AuthorityDecisionRequestInput<'_>,
) -> Result<PrivateExecutionEvent> {
	validate_authority_decision_request_input(&input)?;

	let options = input
		.options
		.iter()
		.map(|option| {
			json!({
				"label": option.label,
				"description": option.description,
			})
		})
		.collect::<Vec<_>>();
	let payload = json!({
		"schema": AUTHORITY_DECISION_REQUEST_SCHEMA,
		"record_version": 1,
		"decision_request_id": input.decision_request_id,
		"issue": {
			"id": input.issue_id,
			"identifier": input.issue_identifier,
		},
		"run": {
			"run_id": input.run_id,
			"attempt_number": input.attempt_number,
		},
		"authority_boundary_check": {
			"record_id": input.boundary_check_record_id,
			"event_type": AUTHORITY_BOUNDARY_CHECK_EVENT_TYPE,
		},
		"phase": "human_required",
		"reason": input.reason_code,
		"boundary": input.boundary_type,
		"proposed_change": input.proposed_change,
		"why_exceeds_authority": input.why_exceeds_authority,
		"options": options,
		"recommendation": input.recommendation,
		"resume_condition": input.resume_condition,
		"next_action": input.resume_condition,
		"retained_worktree_evidence": input.retained_worktree_evidence,
		"retained_diff_evidence": input.retained_diff_evidence,
		"recovery_attempt_context": input.recovery_attempt_context,
	});

	state_store.append_private_execution_event(
		input.project_id,
		input.issue_id,
		input.run_id,
		input.attempt_number,
		AUTHORITY_DECISION_REQUEST_EVENT_TYPE,
		payload,
	)
}

fn operator_execution_program_status(
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

fn operator_execution_program_unknown_status() -> String {
	String::from("unknown")
}

fn operator_execution_program_node_should_render(
	node: &ExecutionNodeEvaluation,
) -> bool {
	node.dispatch_action().is_some()
		|| matches!(
			node.lifecycle_state(),
			crate::execution_program::ExecutionProgramNodeLifecycleState::Active
				| crate::execution_program::ExecutionProgramNodeLifecycleState::Blocked
				| crate::execution_program::ExecutionProgramNodeLifecycleState::Mapped
				| crate::execution_program::ExecutionProgramNodeLifecycleState::NeedsAttention
				| crate::execution_program::ExecutionProgramNodeLifecycleState::Planned
				| crate::execution_program::ExecutionProgramNodeLifecycleState::Stale
				| crate::execution_program::ExecutionProgramNodeLifecycleState::Superseded
		)
}

fn operator_execution_program_node_readback(
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

fn operator_execution_program_missing_contract_nodes(
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

fn operator_execution_program_mapped_issue_identifiers(
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

fn operator_execution_program_reason_codes(reasons: &[String]) -> Vec<String> {
	let mut seen = BTreeSet::new();

	for reason in reasons {
		seen.insert(operator_execution_program_reason_code(reason).to_owned());
	}

	seen.into_iter().collect()
}

fn operator_execution_program_reason_code(reason: &str) -> &'static str {
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

fn operator_execution_program_public_reason(reason: &str) -> String {
	if reason.starts_with("conflict domain `") {
		String::from("another active or retained program node occupies this conflict domain")
	} else if reason.starts_with("dependency `") {
		String::from("a dependency has not reached a required terminal state")
	} else {
		reason.to_owned()
	}
}

fn operator_execution_program_node_next_action(
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
	)
		|| reason_codes.iter().any(|code| code == "mapped_issue_needs_attention")
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

fn validate_authority_boundary_check_input(
	input: &AuthorityBoundaryCheckInput<'_>,
) -> Result<()> {
	authority_boundary_required("authority boundary project_id", input.project_id)?;
	authority_boundary_required("authority boundary issue_id", input.issue_id)?;
	authority_boundary_required("authority boundary issue_identifier", input.issue_identifier)?;
	authority_boundary_required("authority boundary run_id", input.run_id)?;
	authority_boundary_required(
		"authority boundary attempted_recovery_reason",
		input.attempted_recovery_reason,
	)?;
	authority_boundary_required(
		"authority boundary final_disposition_reason",
		input.final_disposition_reason,
	)?;

	if input.attempt_number < 1 {
		eyre::bail!("Authority boundary attempt_number must be positive.");
	}

	for contract_id in &input.decision_contract_ids {
		authority_boundary_required("authority boundary decision_contract_id", contract_id)?;
	}
	for surface in &input.changed_surfaces {
		authority_boundary_required("authority boundary changed surface", surface.surface)?;
		authority_boundary_required(
			"authority boundary changed surface summary",
			surface.change_summary,
		)?;
	}
	for signal in &input.improvement_signals {
		authority_boundary_required("authority boundary improvement kind", signal.kind)?;
		authority_boundary_required("authority boundary improvement reason_code", signal.reason_code)?;
		authority_boundary_required("authority boundary improvement target", signal.target)?;
		authority_boundary_required(
			"authority boundary improvement recommendation",
			signal.recommendation,
		)?;
	}

	Ok(())
}

fn validate_authority_decision_request_input(
	input: &AuthorityDecisionRequestInput<'_>,
) -> Result<()> {
	authority_boundary_required("authority decision project_id", input.project_id)?;
	authority_boundary_required("authority decision issue_id", input.issue_id)?;
	authority_boundary_required("authority decision issue_identifier", input.issue_identifier)?;
	authority_boundary_required("authority decision run_id", input.run_id)?;
	authority_boundary_required(
		"authority decision decision_request_id",
		input.decision_request_id,
	)?;
	authority_boundary_required("authority decision reason_code", input.reason_code)?;
	authority_boundary_required("authority decision boundary_type", input.boundary_type)?;
	authority_boundary_required("authority decision proposed_change", input.proposed_change)?;
	authority_boundary_required(
		"authority decision why_exceeds_authority",
		input.why_exceeds_authority,
	)?;
	authority_boundary_required("authority decision recommendation", input.recommendation)?;
	authority_boundary_required("authority decision resume_condition", input.resume_condition)?;

	if input.attempt_number < 1 {
		eyre::bail!("Authority decision attempt_number must be positive.");
	}
	if input.boundary_check_record_id < 1 {
		eyre::bail!("Authority decision boundary_check_record_id must be positive.");
	}
	if input.options.is_empty() {
		eyre::bail!("Authority decision options must not be empty.");
	}

	for option in &input.options {
		authority_boundary_required("authority decision option label", option.label)?;
		authority_boundary_required("authority decision option description", option.description)?;
	}
	for evidence in &input.retained_worktree_evidence {
		authority_boundary_required("authority decision retained_worktree_evidence", evidence)?;
	}
	for evidence in &input.retained_diff_evidence {
		authority_boundary_required("authority decision retained_diff_evidence", evidence)?;
	}
	for context in &input.recovery_attempt_context {
		authority_boundary_required("authority decision recovery_attempt_context", context)?;
	}

	Ok(())
}

fn authority_boundary_required(name: &str, value: &str) -> Result<()> {
	if value.trim().is_empty() {
		eyre::bail!("{name} must not be empty.");
	}

	Ok(())
}

fn classify_pull_request_readback_report(error: &Report) -> PullRequestReadbackRootCause {
	if report_has_io_error_kind(error, ErrorKind::NotFound) {
		return PullRequestReadbackRootCause::MissingGithubCli;
	}
	if report_contains_any(
		error,
		&[
			"must be configured for this github-backed operation",
			"failed to read environment variable",
			"must not be blank",
		],
	) {
		return PullRequestReadbackRootCause::MissingGithubToken;
	}
	if report_chain_has_serde_json_error(error) {
		return PullRequestReadbackRootCause::GithubResponseParseFailed;
	}
	if report_contains_any(
		error,
		&[
			"pull request url",
			"did not include a repository",
			"did not include a pull request",
			"without an end cursor",
		],
	) {
		return PullRequestReadbackRootCause::PullRequestShapeReadFailed;
	}
	if report_contains_any(
		error,
		&[
			"bad credentials",
			"requires authentication",
			"authentication required",
			"not logged in",
			"gh auth login",
			"http 401",
			"http 403",
		],
	) {
		return PullRequestReadbackRootCause::GithubAuthFailed;
	}

	PullRequestReadbackRootCause::GithubApiReadFailed
}

fn report_has_io_error_kind(error: &Report, kind: ErrorKind) -> bool {
	error.chain().any(|cause| {
		cause
			.downcast_ref::<std::io::Error>()
			.is_some_and(|error| error.kind() == kind)
	})
}

fn report_chain_has_serde_json_error(error: &Report) -> bool {
	error.chain().any(|cause| cause.downcast_ref::<serde_json::Error>().is_some())
}

fn report_contains_any(error: &Report, needles: &[&str]) -> bool {
	error.chain().any(|cause| {
		let message = cause.to_string().to_ascii_lowercase();

		needles.iter().any(|needle| message.contains(needle))
	})
}
