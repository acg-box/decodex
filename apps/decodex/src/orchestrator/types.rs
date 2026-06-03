use crate::tracker;

type PullRequestReadbackResult =
	std::result::Result<PullRequestReviewState, PullRequestReadbackFailure>;

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
	Retry,
	ReviewRepair,
	Closeout,
}
impl IssueDispatchMode {
	fn as_str(self) -> &'static str {
		match self {
			Self::Normal => "normal",
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

pub(crate) enum RetryDispatchDecision {
	Blocked { excluded_issue_ids: Vec<String> },
	Dispatch(Box<RunSummary>),
	Continue,
}

#[derive(Clone, Debug)]
pub(crate) enum ActiveRunDisposition {
	RetainedReviewComplete,
	Superseded {
		newer_run_id: String,
		newer_attempt_number: i64,
	},
	Terminal,
	NonActive,
	Stalled { idle_for: Duration },
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
	pub(crate) allow_unverified_codex: bool,
}

/// Multi-project local control-plane daemon request.
pub(crate) struct ServeRequest<'a> {
	pub(crate) config_path: Option<&'a Path>,
	pub(crate) listen_address: &'a str,
	pub(crate) dev: bool,
	pub(crate) allow_unverified_codex: bool,
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

/// Active lane steer request.
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

/// Active lane steer result without raw operator message content.
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
	active_issues: Vec<TrackerIssue>,
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
	allow_unverified_codex: bool,
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
	allow_unverified_codex: bool,
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
}
impl Display for RetainedPartialProgress {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		write!(
			f,
			"Run `{}` for issue `{}` retained tracked worktree changes at `{}` after failing before terminal handoff; stop automatic retries and finish recovery manually.",
			self.run_id, self.issue_identifier, self.worktree_path
		)
	}
}

impl Error for RetainedPartialProgress {}

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
			"Run `{}` for issue `{}` stalled after {:?} without app-server activity; stop automatic execution and repair manually.",
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
struct ActiveChildRunContext<'a> {
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
struct ActiveRunReconciliation {
	issue: TrackerIssue,
	run_attempt: RunAttempt,
	worktree_mapping: Option<WorktreeMapping>,
	disposition: ActiveRunDisposition,
	workflow: WorkflowDocument,
}

struct TerminalFailureOutcome {
	error_class: &'static str,
	retry_guarded_by_state: bool,
}

#[derive(Clone, Debug, Serialize)]
struct OperatorStatusSnapshot {
	project_id: String,
	run_limit: usize,
	warnings: Vec<String>,
	warning_details: Vec<OperatorSnapshotWarningDetail>,
	connector_backoffs: Vec<OperatorConnectorBackoffStatus>,
	projects: Vec<OperatorProjectStatus>,
	account_control: OperatorCodexAccountControlStatus,
	accounts: Vec<CodexAccountActivitySummary>,
	active_runs: Vec<OperatorRunStatus>,
	recent_runs: Vec<OperatorRunStatus>,
	history_lanes: Vec<OperatorHistoryLaneStatus>,
	queued_candidates: Vec<OperatorQueuedIssueStatus>,
	worktrees: Vec<OperatorWorktreeStatus>,
	post_review_lanes: Vec<OperatorPostReviewLaneStatus>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct OperatorSnapshotWarningDetail {
	warning: String,
	project_id: Option<String>,
	repo_root: Option<String>,
	reason: String,
	next_action: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct OperatorProjectStatus {
	project_id: String,
	config_path: String,
	repo_root: String,
	enabled: bool,
	github_cli_authority: OperatorGitHubCliAuthority,
	active_run_count: usize,
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct OperatorGitHubCliAuthority {
	command_path: String,
	resolved_path: Option<String>,
	configured_path: Option<String>,
	discovery_tier: String,
	available: bool,
	next_action: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct OperatorCodexAccountControlStatus {
	mode: String,
	account_selector: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct OperatorHistoryLaneStatus {
	project_id: String,
	issue_id: String,
	issue_identifier: Option<String>,
	title: Option<String>,
	author: Option<String>,
	issue_key: String,
	attempt_count: usize,
	ledger_outcome: OperatorHistoryLedgerOutcome,
	latest_run: OperatorRunStatus,
	attempts: Vec<OperatorRunStatus>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct OperatorRunStatus {
	project_id: String,
	project_display_name: String,
	run_id: String,
	issue_id: String,
	issue_identifier: Option<String>,
	title: Option<String>,
	author: Option<String>,
	attempt_number: i64,
	status: String,
	attempt_status: String,
	phase: String,
	wait_reason: Option<String>,
	current_operation: String,
	thread_id: Option<String>,
	turn_id: Option<String>,
	thread_status: Option<String>,
	thread_active_flags: Vec<String>,
	interactive_requested: bool,
	continuation_pending: bool,
	active_lease: bool,
	queue_lease_state: String,
	execution_liveness: String,
	updated_at: String,
	last_run_activity_at: Option<String>,
	last_protocol_activity_at: Option<String>,
	last_progress_at: Option<String>,
	idle_for_seconds: Option<i64>,
	protocol_idle_for_seconds: Option<i64>,
	suspected_stall: bool,
	last_event_type: Option<String>,
	last_event_at: Option<String>,
	event_count: i64,
	private_evidence: AgentPrivateEvidenceRef,
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
	account: Option<CodexAccountActivitySummary>,
	accounts: Vec<CodexAccountActivitySummary>,
	branch_name: Option<String>,
	worktree_path: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct OperatorQueuedIssueStatus {
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct OperatorQueuedIssueAttentionStatus {
	summary: String,
	run_id: Option<String>,
	attempt_number: Option<i64>,
	current_operation: Option<String>,
	thread_status: Option<String>,
	attempt_status: Option<String>,
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct OperatorWorktreeStatus {
	issue_id: String,
	issue_identifier: Option<String>,
	issue_state: Option<String>,
	branch_name: String,
	worktree_path: String,
	ownership: String,
	ownership_reason: String,
	hygiene: Option<OperatorWorktreeHygieneStatus>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct OperatorWorktreeHygieneStatus {
	classification: String,
	default_branch: String,
	dirty: bool,
	reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct OperatorPostReviewLaneStatus {
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
	readback_warning: Option<String>,
	readback_root_cause: Option<String>,
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
	allow_unverified_codex: bool,
}

struct ConcurrencySnapshot {
	total_active: usize,
}
impl ConcurrencySnapshot {
	fn new(project_id: &str, state_store: &StateStore) -> crate::prelude::Result<Self> {
		let leases = state_store.list_active_shared_leases(project_id)?;

		Ok(Self { total_active: leases.len() })
	}

	fn has_global_capacity(&self, execution: &WorkflowExecution) -> bool {
		execution.max_concurrent_agents().has_capacity(self.total_active)
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
