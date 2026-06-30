use super::{
	AgentGitCredentialsUnavailable, AppServerCapabilityPreflightFailure,
	AppServerDynamicToolFailure, AppServerHomePreflightFailure, AppServerPhaseGoalFailure,
	AppServerTransportFailure, AppServerTurnFailure, AuthorityBoundaryPolicyDecision,
	CodexAccountAuthFailure, Command, Display, Error, Formatter, HarnessOutcomeKind,
	IssueDispatchMode, IssueRunPlan, IssueTracker, LoopGuardrailCheckpoint,
	LoopGuardrailCheckpointInput, LoopGuardrailReason, LoopGuardrailStopRequested,
	ManualAttentionRequested, OffsetDateTime, Path, PhaseAcceptanceCheckFailure, RepoGateFailure,
	RepoGateFailureDiagnostic, RepoGateFailureDisposition, Report, Result, RetainedPartialProgress,
	RetainedReviewNeedsAttention, RetryComment, RetryKind, ReviewHandoffMarker,
	ReviewHandoffNeedsAttention, ReviewOrchestrationMarker, ReviewPolicyStopReason,
	ReviewPolicyStopRequested, RunCompletionDisposition, ServiceConfig, Sha256,
	StalledRunNeedsAttention, StateStore, TERMINAL_GUARDED_RUN_STATUS, TerminalFailureLifecycle,
	TerminalFailureOutcome, TrackerIssue, WorkflowDocument,
	architecture_recovery_retry_next_action, configured_public_projection_privacy_classifier, env,
	eyre, format_retry_comment, format_terminal_failure_comment, json,
	latest_open_issue_phase_goal_before_attempt, loop_guardrail_architecture_recovery_decision,
	record_harness_outcome_best_effort, records, relative_worktree_path,
	repo_gate_changed_tracked_files, retry_comment_details, retry_delay, slice, state,
	terminal_failure_comment_details, terminal_failure_lifecycle_event, terminal_failure_pr_url,
	terminal_failure_recovery_gate, tracker, worktree_has_tracked_changes, worktree_head_oid,
	write_retry_budget_marker, write_terminal_guard_marker,
};

use records::LinearExecutionEventPublicProjection;
use sha2::Digest;

use crate::tracker::privacy_classifier::PublicProjectionPrivacyClassifier;

mod loop_guardrail;
mod terminal_writeback;

pub(super) use loop_guardrail::{
	git_guardrail_output, loop_guardrail_effective_status, loop_guardrail_stop_from_review_policy,
	loop_guardrail_text_hash, loop_guardrail_worktree_fingerprint,
	retryable_failure_loop_guardrail_stop, run_failure_requires_terminal_attention,
};
pub(super) use terminal_writeback::apply_terminal_failure_writeback;

pub(super) const LOOP_GUARDRAIL_CONVERGENCE_BUDGET: i64 = 3;
pub(super) const ARCHITECTURE_RECOVERY_BUDGET: usize = 1;
pub(super) const ARCHITECTURE_RECOVERY_RETRY_KIND: &str = "architecture_recovery";
pub(super) const REVIEW_HANDOFF_STATE_DRIFT_DETECTED_EVENT_TYPE: &str =
	"review_handoff_state_drift_detected";
pub(super) const REVIEW_HANDOFF_STATE_DRIFT_RECOVERED_EVENT_TYPE: &str =
	"review_handoff_state_drift_recovered";
pub(super) const REVIEW_HANDOFF_REBOUND_ORCHESTRATION_PHASE: &str = "request_pending";
pub(super) const RETRYABLE_FAILED_START_CLEANUP_EVENT_TYPE: &str = "retryable_failed_start_cleanup";

#[derive(Debug)]
pub(super) struct AppServerZeroEvidenceStartFailure {
	issue_identifier: String,
	run_id: String,
}
impl AppServerZeroEvidenceStartFailure {
	pub(super) fn new(issue_identifier: String, run_id: String) -> Self {
		Self { issue_identifier, run_id }
	}

	pub(super) fn error_class(&self) -> &'static str {
		"app_server_zero_evidence_start_failed"
	}

	pub(super) fn terminal_next_action(&self, recovery_gate: &str) -> String {
		format!(
			"inspect local app-server startup logs and Decodex account/runtime state for run `{}`, verify `decodex probe stdio://`, restart `decodex serve` if needed, {recovery_gate}",
			self.run_id
		)
	}

	pub(super) fn retry_next_action(&self) -> String {
		format!(
			"restart the app-server and retry automatically for run `{}`; inspect private startup diagnostics if the retry budget exhausts",
			self.run_id
		)
	}
}

impl Display for AppServerZeroEvidenceStartFailure {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		write!(
			formatter,
			"App-server run `{}` for issue `{}` failed before Decodex recorded a thread, turn, protocol event, or private execution event.",
			self.run_id, self.issue_identifier
		)
	}
}

impl Error for AppServerZeroEvidenceStartFailure {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RetainedReviewRepairPushFailureKind {
	Auth,
	Refspec,
	RemoteRejected,
	Failed,
}
impl RetainedReviewRepairPushFailureKind {
	pub(super) fn error_class(self) -> &'static str {
		match self {
			Self::Auth => "retained_review_repair_push_auth_failed",
			Self::Refspec => "retained_review_repair_push_refspec_failed",
			Self::RemoteRejected => "retained_review_repair_push_remote_rejected",
			Self::Failed => "retained_review_repair_push_failed",
		}
	}
}

#[derive(Debug)]
pub(super) struct RetainedReviewRepairPushFailed {
	pub(super) issue_identifier: String,
	pub(super) run_id: String,
	pub(super) branch_name: String,
	pub(super) pr_url: Option<String>,
	pub(super) kind: RetainedReviewRepairPushFailureKind,
	pub(super) detail: String,
}
impl RetainedReviewRepairPushFailed {
	pub(super) fn error_class(&self) -> &'static str {
		self.kind.error_class()
	}

	pub(super) fn terminal_next_action(&self, recovery_gate: &str) -> String {
		match self.kind {
			RetainedReviewRepairPushFailureKind::Auth => format!(
				"repair GitHub authentication, then rerun retained review repair for branch `{}`, {recovery_gate}",
				self.branch_name
			),
			RetainedReviewRepairPushFailureKind::Refspec => format!(
				"inspect retained review-repair branch `{}` and push refspec, repair the branch/ref mismatch manually, {recovery_gate}",
				self.branch_name
			),
			RetainedReviewRepairPushFailureKind::RemoteRejected => format!(
				"inspect remote branch `{}` for non-fast-forward or protection drift, reconcile the retained PR branch, {recovery_gate}",
				self.branch_name
			),
			RetainedReviewRepairPushFailureKind::Failed => format!(
				"inspect the retained review-repair push failure for branch `{}`, repair the remote update blocker, {recovery_gate}",
				self.branch_name
			),
		}
	}
}

impl Display for RetainedReviewRepairPushFailed {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		write!(
			formatter,
			"Run `{}` for issue `{}` could not push retained review-repair branch `{}` before handoff validation: {}",
			self.run_id, self.issue_identifier, self.branch_name, self.detail
		)
	}
}

impl Error for RetainedReviewRepairPushFailed {}
#[derive(Clone, Copy)]
pub(super) struct TerminalFailureWritebackRuntime<'a> {
	pub(super) service_id: &'a str,
	pub(super) state_store: Option<&'a StateStore>,
	pub(super) privacy_classifier: &'a dyn PublicProjectionPrivacyClassifier,
}

struct PreparedTerminalFailureWriteback {
	failure_state_id: String,
	needs_attention_label: String,
	needs_attention_label_id: Option<String>,
	terminal_failure_state_name: String,
	projection: LinearExecutionEventPublicProjection,
	error_class: &'static str,
	retry_guarded_by_state: bool,
}

pub(super) struct ArchitectureRecoveryStart {
	pub(super) attempt_number: usize,
	pub(super) max_attempts: usize,
	pub(super) policy_decision: AuthorityBoundaryPolicyDecision,
	pub(super) detail: String,
}
struct ZeroEvidenceAppServerStartFailureContext {
	protocol_event_count: i64,
	private_event_count: usize,
	thread_recorded: bool,
	turn_recorded: bool,
}

struct ZeroEvidenceAppServerStartFailureDiagnostic {
	source_error_summary: String,
	source_error_chain: Vec<String>,
}

pub(super) struct LoopGuardrailWorktreeFingerprint {
	pub(super) head_sha: String,
	pub(super) tracked_status_hash: String,
	pub(super) tracked_diff_hash: String,
	pub(super) effective_status_hash: String,
	pub(super) branch_delta_present: bool,
	pub(super) effective_delta_present: bool,
}

struct FailureHandlingContext<'a, T>
where
	T: IssueTracker,
{
	tracker: &'a T,
	project: &'a ServiceConfig,
	workflow: &'a WorkflowDocument,
	state_store: &'a StateStore,
	issue_run: &'a IssueRunPlan,
	worktree_path: &'a str,
	retry_budget_attempts: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RunFailureWritebackDisposition {
	RetryableGeneric,
	RetryableStructuredRecovery,
	TerminalAttention,
}
impl RunFailureWritebackDisposition {
	pub(super) fn requires_terminal_attention(self) -> bool {
		self == Self::TerminalAttention
	}

	pub(super) fn preserves_retry_through_zero_evidence(self) -> bool {
		self == Self::RetryableStructuredRecovery
	}
}
pub(super) enum LoopGuardrailRecoveryDecision {
	Start(ArchitectureRecoveryStart),
	HumanRequired(LoopGuardrailStopRequested),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReviewHandoffFailureDriftLineage {
	Exact,
	Descends,
	Diverged,
	Unknown,
}
impl ReviewHandoffFailureDriftLineage {
	fn allows_lifecycle_recovery(self) -> bool {
		matches!(self, Self::Exact | Self::Descends)
	}

	fn as_str(self) -> &'static str {
		match self {
			Self::Exact => "exact",
			Self::Descends => "descends",
			Self::Diverged => "diverged",
			Self::Unknown => "unknown",
		}
	}
}

enum ReviewHandoffStateDriftTransition {
	AlreadySuccess,
	MoveToSuccess(String),
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum TerminalFailureEventRecordStatus {
	Recorded,
	Duplicate,
	NoLocalStore,
}

pub(super) fn run_failure_writeback_disposition(error: &Report) -> RunFailureWritebackDisposition {
	if error.downcast_ref::<ManualAttentionRequested>().is_some()
		|| error.downcast_ref::<LoopGuardrailStopRequested>().is_some()
		|| error
			.downcast_ref::<AppServerPhaseGoalFailure>()
			.is_some_and(|failure| !failure.is_terminal_path_missing())
		|| error.downcast_ref::<ReviewHandoffNeedsAttention>().is_some()
		|| error.downcast_ref::<RetainedReviewRepairPushFailed>().is_some()
		|| error.downcast_ref::<RetainedPartialProgress>().is_some()
		|| error
			.downcast_ref::<AppServerCapabilityPreflightFailure>()
			.is_some_and(|failure| !failure.is_retryable_timeout())
		|| error.downcast_ref::<AppServerHomePreflightFailure>().is_some()
		|| error.downcast_ref::<CodexAccountAuthFailure>().is_some()
		|| error
			.downcast_ref::<AppServerTransportFailure>()
			.is_some_and(|failure| !failure.is_retryable_startup())
		|| error.downcast_ref::<AgentGitCredentialsUnavailable>().is_some()
		|| error
			.downcast_ref::<AppServerTurnFailure>()
			.is_some_and(AppServerTurnFailure::requires_operator_attention)
		|| error.downcast_ref::<ReviewPolicyStopRequested>().is_some()
		|| error.downcast_ref::<RepoGateFailure>().is_some_and(|repo_gate_failure| {
			repo_gate_failure.disposition() == RepoGateFailureDisposition::NeedsHumanAttention
		}) {
		RunFailureWritebackDisposition::TerminalAttention
	} else if error.downcast_ref::<AppServerZeroEvidenceStartFailure>().is_some()
		|| error
			.downcast_ref::<AppServerCapabilityPreflightFailure>()
			.is_some_and(AppServerCapabilityPreflightFailure::is_retryable_timeout)
		|| error.downcast_ref::<StalledRunNeedsAttention>().is_some()
		|| error.downcast_ref::<RepoGateFailure>().is_some_and(|repo_gate_failure| {
			matches!(
				repo_gate_failure.disposition(),
				RepoGateFailureDisposition::ContinueRepair
					| RepoGateFailureDisposition::RetryAfterBackoff
			)
		}) || error
		.downcast_ref::<AppServerTransportFailure>()
		.is_some_and(AppServerTransportFailure::is_retryable_startup)
		|| error
			.downcast_ref::<AppServerPhaseGoalFailure>()
			.is_some_and(AppServerPhaseGoalFailure::is_terminal_path_missing)
		|| error.downcast_ref::<AppServerDynamicToolFailure>().is_some()
		|| error.downcast_ref::<AppServerTurnFailure>().is_some()
	{
		RunFailureWritebackDisposition::RetryableStructuredRecovery
	} else {
		RunFailureWritebackDisposition::RetryableGeneric
	}
}

pub(super) fn preserve_manual_attention_request(
	completion_disposition: Result<RunCompletionDisposition>,
	issue_run: &IssueRunPlan,
	workflow: &WorkflowDocument,
	error: Report,
) -> Report {
	if matches!(completion_disposition, Ok(RunCompletionDisposition::ManualAttention)) {
		return Report::new(ManualAttentionRequested {
			issue_identifier: issue_run.issue.identifier.clone(),
			label: workflow.frontmatter().tracker().needs_attention_label().to_owned(),
			run_id: issue_run.run_id.clone(),
			error_class: None,
		})
		.wrap_err(error);
	}

	error
}

pub(super) fn preserve_and_promote_app_server_run_failure(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	workflow: &WorkflowDocument,
	completion_disposition: Result<RunCompletionDisposition>,
	error: Report,
) -> Report {
	let error =
		preserve_manual_attention_request(completion_disposition, issue_run, workflow, error);

	promote_zero_evidence_app_server_start_failure(project, state_store, issue_run, error)
}

pub(super) fn promote_zero_evidence_app_server_start_failure(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	error: Report,
) -> Report {
	let writeback_disposition = run_failure_writeback_disposition(&error);

	if writeback_disposition.requires_terminal_attention()
		|| writeback_disposition.preserves_retry_through_zero_evidence()
	{
		return error;
	}

	match zero_evidence_app_server_start_failure_context(project, state_store, issue_run) {
		Ok(Some(context)) => {
			let diagnostic = zero_evidence_app_server_start_failure_diagnostic(&error);

			if let Err(record_error) = record_zero_evidence_app_server_start_failure(
				project,
				state_store,
				issue_run,
				&context,
				&diagnostic,
			) {
				tracing::warn!(
					?record_error,
					project_id = project.service_id(),
					issue_id = issue_run.issue.id,
					issue = issue_run.issue.identifier,
					run_id = issue_run.run_id,
					attempt = issue_run.attempt_number,
					"Failed to record zero-evidence app-server start failure evidence."
				);
			}

			Report::new(AppServerZeroEvidenceStartFailure::new(
				issue_run.issue.identifier.clone(),
				issue_run.run_id.clone(),
			))
			.wrap_err(error)
		},
		Ok(None) => error,
		Err(context_error) => {
			tracing::warn!(
				?context_error,
				project_id = project.service_id(),
				issue_id = issue_run.issue.id,
				issue = issue_run.issue.identifier,
				run_id = issue_run.run_id,
				attempt = issue_run.attempt_number,
				"Failed to classify app-server start failure evidence."
			);

			error
		},
	}
}

fn zero_evidence_app_server_start_failure_context(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
) -> Result<Option<ZeroEvidenceAppServerStartFailureContext>> {
	let protocol_event_count = state_store.event_count(&issue_run.run_id)?;
	let private_event_count = state_store
		.list_private_execution_events(
			project.service_id(),
			&issue_run.issue.id,
			&issue_run.run_id,
			issue_run.attempt_number,
		)?
		.len();
	let run_attempt = state_store.run_attempt(&issue_run.run_id)?;
	let thread_recorded = run_attempt.as_ref().and_then(|attempt| attempt.thread_id()).is_some();
	let turn_recorded = run_attempt.as_ref().and_then(|attempt| attempt.turn_id()).is_some();

	if protocol_event_count == 0 && private_event_count == 0 && !thread_recorded && !turn_recorded {
		Ok(Some(ZeroEvidenceAppServerStartFailureContext {
			protocol_event_count,
			private_event_count,
			thread_recorded,
			turn_recorded,
		}))
	} else {
		Ok(None)
	}
}

fn record_zero_evidence_app_server_start_failure(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	context: &ZeroEvidenceAppServerStartFailureContext,
	diagnostic: &ZeroEvidenceAppServerStartFailureDiagnostic,
) -> Result<()> {
	state_store
		.append_private_execution_event(
			project.service_id(),
			&issue_run.issue.id,
			&issue_run.run_id,
			issue_run.attempt_number,
			"app_server_zero_evidence_start_failure",
			json!({
				"error_class": "app_server_zero_evidence_start_failed",
				"summary": "App-server dispatch failed before Decodex recorded a thread, turn, protocol event, or private execution event.",
				"issue_identifier": issue_run.issue.identifier.as_str(),
				"attempt_number": issue_run.attempt_number,
				"branch": issue_run.worktree.branch_name.as_str(),
				"worktree_path": issue_run.worktree.path.display().to_string(),
				"protocol_event_count": context.protocol_event_count,
				"private_event_count": context.private_event_count,
				"thread_recorded": context.thread_recorded,
				"turn_recorded": context.turn_recorded,
				"source_error_summary": diagnostic.source_error_summary.as_str(),
				"source_error_chain": &diagnostic.source_error_chain,
			}),
		)
		.map(|_| ())
}

fn zero_evidence_app_server_start_failure_diagnostic(
	error: &Report,
) -> ZeroEvidenceAppServerStartFailureDiagnostic {
	let source_error_chain = error
		.chain()
		.map(|cause| sanitize_private_diagnostic_text(&cause.to_string()))
		.collect::<Vec<_>>();
	let source_error_summary = source_error_chain
		.first()
		.cloned()
		.unwrap_or_else(|| String::from("unknown app-server startup failure"));

	ZeroEvidenceAppServerStartFailureDiagnostic { source_error_summary, source_error_chain }
}

fn sanitize_private_diagnostic_text(text: &str) -> String {
	let mut sanitized = text.to_owned();

	for (name, value) in env::vars() {
		if !diagnostic_env_var_name_is_sensitive(&name) || value.len() < 6 {
			continue;
		}

		let replacement = format!("<redacted env:{name}>");

		sanitized = sanitized.replace(&value, &replacement);
	}

	truncate_private_diagnostic_text(&sanitized)
}

fn diagnostic_env_var_name_is_sensitive(name: &str) -> bool {
	let normalized = name.to_ascii_lowercase();

	normalized.contains("token")
		|| normalized.contains("secret")
		|| normalized.contains("password")
		|| normalized.contains("credential")
		|| normalized.contains("api_key")
		|| normalized.contains("apikey")
		|| normalized.ends_with("_pat")
		|| normalized.starts_with("pat_")
		|| normalized.contains("_pat_")
		|| normalized.contains("auth")
}

pub(super) fn truncate_private_diagnostic_text(text: &str) -> String {
	const MAX_PRIVATE_DIAGNOSTIC_TEXT_CHARS: usize = 2_000;

	if text.chars().count() <= MAX_PRIVATE_DIAGNOSTIC_TEXT_CHARS {
		return text.to_owned();
	}

	let mut truncated = text.chars().take(MAX_PRIVATE_DIAGNOSTIC_TEXT_CHARS).collect::<String>();

	truncated.push_str("...<truncated>");

	truncated
}

pub(super) fn try_recover_review_handoff_failure_drift<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	error: &Report,
) -> Result<bool>
where
	T: IssueTracker,
{
	if !review_handoff_failure_drift_can_handle(error) {
		return Ok(false);
	}

	let Some(worktree_fingerprint) = loop_guardrail_worktree_fingerprint(&issue_run.worktree.path)?
	else {
		return Ok(false);
	};

	if worktree_fingerprint.effective_delta_present {
		return Ok(false);
	}

	let Some(review_handoff) = state_store.review_handoff_marker(
		project.service_id(),
		&issue_run.issue.id,
		&issue_run.worktree.branch_name,
	)?
	else {
		return Ok(false);
	};

	if review_handoff.branch_name() != issue_run.worktree.branch_name
		|| review_handoff.pr_head_ref_name() != issue_run.worktree.branch_name
	{
		return Ok(false);
	}

	let lineage = review_handoff_failure_drift_lineage(
		&issue_run.worktree.path,
		review_handoff.pr_head_oid(),
		&worktree_fingerprint.head_sha,
	);

	if !lineage.allows_lifecycle_recovery() {
		return Ok(false);
	}

	let tracker_policy = workflow.frontmatter().tracker();
	let success_state = tracker_policy.success_state();
	let current_state = issue_run.issue.state.name.as_str();
	let Some(success_state_transition) =
		review_handoff_state_drift_success_transition(workflow, issue_run)?
	else {
		return Ok(false);
	};
	let issue_state_recovered =
		matches!(success_state_transition, ReviewHandoffStateDriftTransition::MoveToSuccess(_));
	let rebounded_orchestration = rebound_review_handoff_orchestration_marker(
		project,
		state_store,
		issue_run,
		&review_handoff,
		&worktree_fingerprint.head_sha,
	)?;
	let needs_attention_cleared = tracker::set_issue_label_presence(
		tracker,
		&issue_run.issue,
		tracker_policy.needs_attention_label(),
		false,
	)?;

	if let ReviewHandoffStateDriftTransition::MoveToSuccess(state_id) = success_state_transition {
		tracker.update_issue_state(&issue_run.issue.id, &state_id)?;
	}

	state_store
		.clear_loop_guardrail_checkpoints_for_issue(project.service_id(), &issue_run.issue.id)?;
	state_store.update_run_status(&issue_run.run_id, "succeeded")?;
	state_store
		.append_private_execution_event(
			project.service_id(),
			&issue_run.issue.id,
			&issue_run.run_id,
			issue_run.attempt_number,
			REVIEW_HANDOFF_STATE_DRIFT_RECOVERED_EVENT_TYPE,
			json!({
				"schema": "decodex.review_handoff_state_drift_recovered/1",
				"reason": "current_review_handoff_marker",
				"source_error_class": review_handoff_failure_drift_source_error_class(error),
				"branch_name": issue_run.worktree.branch_name,
				"pr_url": review_handoff.pr_url(),
				"marker_head_sha": review_handoff.pr_head_oid(),
				"local_head_sha": worktree_fingerprint.head_sha,
				"lineage": lineage.as_str(),
				"previous_issue_state": current_state,
				"target_issue_state": success_state,
				"issue_state_recovered": issue_state_recovered,
				"needs_attention_cleared": needs_attention_cleared,
				"orchestration_rebound": rebounded_orchestration,
			}),
		)
		.map(|_| ())?;

	tracing::warn!(
		project_id = project.service_id(),
		issue_id = issue_run.issue.id,
		issue = issue_run.issue.identifier,
		run_id = issue_run.run_id,
		attempt = issue_run.attempt_number,
		branch = issue_run.worktree.branch_name,
		pr_url = review_handoff.pr_url(),
		lineage = lineage.as_str(),
		"Recovered review handoff state drift before retry/no-diff failure writeback."
	);

	Ok(true)
}

fn review_handoff_state_drift_success_transition(
	workflow: &WorkflowDocument,
	issue_run: &IssueRunPlan,
) -> Result<Option<ReviewHandoffStateDriftTransition>> {
	let tracker_policy = workflow.frontmatter().tracker();
	let success_state = tracker_policy.success_state();
	let current_state = issue_run.issue.state.name.as_str();

	if current_state == success_state {
		return Ok(Some(ReviewHandoffStateDriftTransition::AlreadySuccess));
	}
	if current_state != tracker_policy.in_progress_state()
		&& current_state != tracker_policy.failure_state()
	{
		return Ok(None);
	}

	let state_id = issue_run.issue.state_id_for_name(success_state).ok_or_else(|| {
		eyre::eyre!(
			"State `{success_state}` was not found for issue `{}` during review handoff state drift recovery.",
			issue_run.issue.identifier
		)
	})?;

	Ok(Some(ReviewHandoffStateDriftTransition::MoveToSuccess(state_id.to_owned())))
}

fn rebound_review_handoff_orchestration_marker(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	review_handoff: &ReviewHandoffMarker,
	local_head_sha: &str,
) -> Result<bool> {
	let existing_orchestration = state_store.review_orchestration_marker(
		project.service_id(),
		&issue_run.issue.id,
		review_handoff,
	)?;
	let rebounded_orchestration = existing_orchestration.as_ref().is_none_or(|marker| {
		marker.branch_name() != review_handoff.branch_name()
			|| marker.pr_url() != review_handoff.pr_url()
			|| marker.head_sha() != local_head_sha
			|| marker.phase() != REVIEW_HANDOFF_REBOUND_ORCHESTRATION_PHASE
	});
	let orchestration_marker = ReviewOrchestrationMarker::new(
		review_handoff.run_id().to_owned(),
		review_handoff.attempt_number(),
		review_handoff.branch_name().to_owned(),
		review_handoff.pr_url().to_owned(),
		local_head_sha.to_owned(),
		REVIEW_HANDOFF_REBOUND_ORCHESTRATION_PHASE,
		None,
		None,
		None,
		0,
		existing_orchestration.as_ref().map_or(0, ReviewOrchestrationMarker::external_round_count),
		None,
	);

	state_store.upsert_review_orchestration_marker(
		project.service_id(),
		&issue_run.issue.id,
		&orchestration_marker,
	)?;

	Ok(rebounded_orchestration)
}

fn review_handoff_failure_drift_can_handle(error: &Report) -> bool {
	!run_failure_requires_terminal_attention(error)
		&& error.downcast_ref::<ManualAttentionRequested>().is_none()
		&& error.downcast_ref::<LoopGuardrailStopRequested>().is_none()
		&& error.downcast_ref::<ReviewHandoffNeedsAttention>().is_none()
		&& error.downcast_ref::<RetainedReviewNeedsAttention>().is_none()
		&& error.downcast_ref::<ReviewPolicyStopRequested>().is_none()
		&& error.downcast_ref::<CodexAccountAuthFailure>().is_none()
}

fn review_handoff_failure_drift_source_error_class(error: &Report) -> &'static str {
	retained_progress_source_error_class(error).unwrap_or("retryable_execution_failure")
}

fn review_handoff_failure_drift_lineage(
	worktree_path: &Path,
	recorded_head_oid: &str,
	local_head_oid: &str,
) -> ReviewHandoffFailureDriftLineage {
	if recorded_head_oid == local_head_oid {
		return ReviewHandoffFailureDriftLineage::Exact;
	}

	let Ok(output) = Command::new("git")
		.arg("-C")
		.arg(worktree_path)
		.args(["merge-base", "--is-ancestor", recorded_head_oid, local_head_oid])
		.output()
	else {
		return ReviewHandoffFailureDriftLineage::Unknown;
	};

	match output.status.code() {
		Some(0) => ReviewHandoffFailureDriftLineage::Descends,
		Some(1) => ReviewHandoffFailureDriftLineage::Diverged,
		_ => ReviewHandoffFailureDriftLineage::Unknown,
	}
}

fn review_handoff_state_drift_attention_error(
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	error: &Report,
) -> Result<Option<ManualAttentionRequested>> {
	if !review_handoff_failure_drift_can_handle(error) {
		return Ok(None);
	}

	let Some(worktree_fingerprint) = loop_guardrail_worktree_fingerprint(&issue_run.worktree.path)?
	else {
		return Ok(None);
	};

	if worktree_fingerprint.effective_delta_present {
		return Ok(None);
	}

	let checkpoint = state_store.review_policy_checkpoint(
		project.service_id(),
		&issue_run.issue.id,
		&issue_run.run_id,
		issue_run.attempt_number,
		"handoff",
	)?;
	let drift_reason = match state_store.review_handoff_marker(
		project.service_id(),
		&issue_run.issue.id,
		&issue_run.worktree.branch_name,
	)? {
		Some(review_handoff) => review_handoff_marker_drift_reason(
			workflow,
			issue_run,
			&worktree_fingerprint,
			&review_handoff,
		)?,
		None => {
			let Some(checkpoint) = checkpoint.as_ref() else {
				return Ok(None);
			};

			if checkpoint.status() != "clean"
				|| checkpoint.head_sha() != worktree_fingerprint.head_sha
			{
				return Ok(None);
			}

			Some(String::from("missing_review_handoff_marker"))
		},
	};
	let Some(drift_reason) = drift_reason else {
		return Ok(None);
	};

	state_store
		.append_private_execution_event(
			project.service_id(),
			&issue_run.issue.id,
			&issue_run.run_id,
			issue_run.attempt_number,
			REVIEW_HANDOFF_STATE_DRIFT_DETECTED_EVENT_TYPE,
			json!({
				"schema": "decodex.review_handoff_state_drift_detected/1",
				"reason": drift_reason,
				"source_error_class": review_handoff_failure_drift_source_error_class(error),
				"branch_name": issue_run.worktree.branch_name,
				"checkpoint_status": checkpoint.as_ref().map(|checkpoint| checkpoint.status()),
				"checkpoint_head_sha": checkpoint.as_ref().map(|checkpoint| checkpoint.head_sha()),
				"local_head_sha": worktree_fingerprint.head_sha,
				"next_action": "restore or rebind the retained review handoff marker before retrying execution",
			}),
		)
		.map(|_| ())?;

	Ok(Some(ManualAttentionRequested {
		issue_identifier: issue_run.issue.identifier.clone(),
		label: workflow.frontmatter().tracker().needs_attention_label().to_owned(),
		run_id: issue_run.run_id.clone(),
		error_class: Some(LoopGuardrailReason::ReviewHandoffStateDrift.error_class().to_owned()),
	}))
}

fn review_handoff_marker_drift_reason(
	workflow: &WorkflowDocument,
	issue_run: &IssueRunPlan,
	worktree_fingerprint: &LoopGuardrailWorktreeFingerprint,
	review_handoff: &ReviewHandoffMarker,
) -> Result<Option<String>> {
	if review_handoff.branch_name() != issue_run.worktree.branch_name {
		return Ok(Some(String::from("review_handoff_marker_branch_mismatch")));
	}
	if review_handoff.pr_head_ref_name() != issue_run.worktree.branch_name {
		return Ok(Some(String::from("review_handoff_marker_pr_head_ref_mismatch")));
	}

	let lineage = review_handoff_failure_drift_lineage(
		&issue_run.worktree.path,
		review_handoff.pr_head_oid(),
		&worktree_fingerprint.head_sha,
	);

	if !lineage.allows_lifecycle_recovery() {
		return Ok(Some(format!("review_handoff_marker_{}", lineage.as_str())));
	}
	if review_handoff_state_drift_success_transition(workflow, issue_run)?.is_some() {
		return Ok(None);
	}

	Ok(Some(String::from("review_handoff_marker_issue_state_unsupported")))
}

pub(super) fn handle_failure<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	error: &Report,
) -> Result<()>
where
	T: IssueTracker,
{
	let max_attempts = i64::from(workflow.frontmatter().execution().max_attempts());
	let manual_attention_requested = error.downcast_ref::<ManualAttentionRequested>().is_some();
	let requires_terminal_attention = run_failure_requires_terminal_attention(error);
	let worktree_path = relative_worktree_path(project, &issue_run.worktree);
	let retry_budget_attempts = retry_budget_attempts_for_current_failure(state_store, issue_run)?;
	let failure_context = FailureHandlingContext {
		tracker,
		project,
		workflow,
		state_store,
		issue_run,
		worktree_path: &worktree_path,
		retry_budget_attempts,
	};

	if handle_review_handoff_failure_drift(
		tracker,
		project,
		workflow,
		state_store,
		issue_run,
		error,
		&worktree_path,
	)? {
		return Ok(());
	}

	let loop_guardrail_stop = retryable_failure_loop_guardrail_stop_unless_terminal_attention(
		project,
		state_store,
		issue_run,
		error,
		requires_terminal_attention,
	)?;
	let retained_partial_progress =
		retained_partial_progress_error(error, issue_run, &worktree_path);

	if let Some(review_policy_stop) = error.downcast_ref::<ReviewPolicyStopRequested>()
		&& review_policy_stop.reason == ReviewPolicyStopReason::Exhausted
	{
		return match loop_guardrail_architecture_recovery_decision(
			project,
			state_store,
			issue_run,
			loop_guardrail_stop_from_review_policy(review_policy_stop),
			error,
		)? {
			LoopGuardrailRecoveryDecision::Start(recovery) => {
				apply_architecture_recovery_retry_writeback(
					&failure_context,
					recovery,
					max_attempts,
				)
			},
			LoopGuardrailRecoveryDecision::HumanRequired(loop_guardrail_stop) => {
				apply_loop_guardrail_failure_writeback(&failure_context, loop_guardrail_stop)
			},
		};
	}
	if let Some(loop_guardrail_stop) = loop_guardrail_stop {
		return match loop_guardrail_architecture_recovery_decision(
			project,
			state_store,
			issue_run,
			loop_guardrail_stop,
			error,
		)? {
			LoopGuardrailRecoveryDecision::Start(recovery) => {
				apply_architecture_recovery_retry_writeback(
					&failure_context,
					recovery,
					max_attempts,
				)
			},
			LoopGuardrailRecoveryDecision::HumanRequired(loop_guardrail_stop) => {
				apply_loop_guardrail_failure_writeback(&failure_context, loop_guardrail_stop)
			},
		};
	}

	if !requires_terminal_attention && retry_budget_attempts < max_attempts {
		return apply_retryable_failure_writeback(&failure_context, error, max_attempts);
	}

	let terminal_error = retained_partial_progress.as_ref().unwrap_or(error);

	apply_terminal_attention_failure_writeback(
		&failure_context,
		manual_attention_requested,
		terminal_error,
	)
}

fn handle_review_handoff_failure_drift<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	error: &Report,
	worktree_path: &str,
) -> Result<bool>
where
	T: IssueTracker,
{
	if try_recover_review_handoff_failure_drift(
		tracker,
		project,
		workflow,
		state_store,
		issue_run,
		error,
	)? {
		return Ok(true);
	}

	let Some(attention_error) = review_handoff_state_drift_attention_error(
		project,
		workflow,
		state_store,
		issue_run,
		error,
	)?
	else {
		return Ok(false);
	};

	apply_review_handoff_state_drift_attention_writeback(
		tracker,
		project,
		workflow,
		state_store,
		issue_run,
		worktree_path,
		attention_error,
	)?;

	Ok(true)
}

fn apply_terminal_attention_failure_writeback<T>(
	context: &FailureHandlingContext<'_, T>,
	manual_attention_requested: bool,
	terminal_error: &Report,
) -> Result<()>
where
	T: IssueTracker,
{
	let privacy_classifier = configured_public_projection_privacy_classifier(context.project)?;
	let outcome = apply_terminal_failure_writeback(
		context.tracker,
		TerminalFailureWritebackRuntime {
			service_id: context.project.service_id(),
			state_store: Some(context.state_store),
			privacy_classifier: &privacy_classifier,
		},
		context.workflow,
		context.issue_run,
		context.worktree_path,
		manual_attention_requested,
		terminal_error,
	)?;

	if outcome.retry_guarded_by_state {
		write_terminal_guard_marker(
			&context.issue_run.worktree.path,
			&context.issue_run.run_id,
			context.issue_run.attempt_number,
		)?;

		context
			.state_store
			.update_run_status(&context.issue_run.run_id, TERMINAL_GUARDED_RUN_STATUS)?;
	}

	write_retry_budget_marker(
		&context.issue_run.worktree.path,
		&context.issue_run.run_id,
		context.issue_run.attempt_number,
		context.retry_budget_attempts,
	)?;

	tracing::warn!(
		project_id = context.project.service_id(),
		issue_id = context.issue_run.issue.id,
		issue = context.issue_run.issue.identifier,
		run_id = context.issue_run.run_id,
		attempt = context.issue_run.attempt_number,
		branch = context.issue_run.worktree.branch_name,
		worktree_path = %context.worktree_path,
		error_class = outcome.error_class,
		"Run failed and now requires operator attention."
	);

	Ok(())
}

pub(super) fn retryable_failure_loop_guardrail_stop_unless_terminal_attention(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	error: &Report,
	requires_terminal_attention: bool,
) -> Result<Option<LoopGuardrailStopRequested>> {
	if requires_terminal_attention {
		Ok(None)
	} else {
		retryable_failure_loop_guardrail_stop(project, state_store, issue_run, error)
	}
}

fn apply_review_handoff_state_drift_attention_writeback<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	worktree_path: &str,
	attention_error: ManualAttentionRequested,
) -> Result<()>
where
	T: IssueTracker,
{
	let terminal_error = Report::new(attention_error);
	let privacy_classifier = configured_public_projection_privacy_classifier(project)?;
	let outcome = apply_terminal_failure_writeback(
		tracker,
		TerminalFailureWritebackRuntime {
			service_id: project.service_id(),
			state_store: Some(state_store),
			privacy_classifier: &privacy_classifier,
		},
		workflow,
		issue_run,
		worktree_path,
		true,
		&terminal_error,
	)?;

	if outcome.retry_guarded_by_state {
		write_terminal_guard_marker(
			&issue_run.worktree.path,
			&issue_run.run_id,
			issue_run.attempt_number,
		)?;

		state_store.update_run_status(&issue_run.run_id, TERMINAL_GUARDED_RUN_STATUS)?;
	}

	Ok(())
}

fn apply_retryable_failure_writeback<T>(
	context: &FailureHandlingContext<'_, T>,
	error: &Report,
	max_attempts: i64,
) -> Result<()>
where
	T: IssueTracker,
{
	let (retry_error_class, retry_next_action) = retry_comment_details(error);

	write_retry_schedule_marker_for_runtime_retry(
		error,
		context.workflow,
		context.issue_run,
		context.retry_budget_attempts,
	)?;

	tracing::warn!(
		project_id = context.project.service_id(),
		issue_id = context.issue_run.issue.id,
		issue = context.issue_run.issue.identifier,
		run_id = context.issue_run.run_id,
		attempt = context.issue_run.attempt_number,
		retry_budget_attempt = context.retry_budget_attempts,
		max_attempts,
		branch = context.issue_run.worktree.branch_name,
		worktree_path = %context.worktree_path,
		error_class = retry_error_class,
		"Run failed and remains retryable."
	);

	tracker::create_public_comment(
		context.tracker,
		&context.issue_run.issue.id,
		&format_retry_comment(RetryComment {
			run_id: &context.issue_run.run_id,
			attempt_number: context.issue_run.attempt_number,
			retry_budget_attempt_number: context.retry_budget_attempts,
			max_attempts,
			worktree_path: context.worktree_path.to_owned(),
			branch_name: &context.issue_run.worktree.branch_name,
			error_class: retry_error_class,
			next_action: &retry_next_action,
		}),
	)?;

	write_retry_budget_marker(
		&context.issue_run.worktree.path,
		&context.issue_run.run_id,
		context.issue_run.attempt_number,
		context.retry_budget_attempts,
	)?;
	record_harness_outcome_best_effort(
		context.state_store,
		context.project.service_id(),
		context.issue_run,
		HarnessOutcomeKind::RetryableFailure,
		Some(retry_error_class),
		retryable_failure_validation_result(error, retry_error_class),
		None,
	);
	cleanup_retryable_failed_start_ownership(context, error)?;

	Ok(())
}

fn cleanup_retryable_failed_start_ownership<T>(
	context: &FailureHandlingContext<'_, T>,
	error: &Report,
) -> Result<()>
where
	T: IssueTracker,
{
	if !retryable_failed_start_cleanup_allowed(context, error)? {
		return Ok(());
	}

	let tracker_policy = context.workflow.frontmatter().tracker();
	let failure_state_name = tracker_policy.failure_state();
	let failure_state_is_startable =
		tracker_policy.startable_states().iter().any(|state| state == failure_state_name);

	if !failure_state_is_startable {
		tracing::warn!(
			issue_id = context.issue_run.issue.id,
			issue = context.issue_run.issue.identifier,
			target_state = failure_state_name,
			"Retryable failed-start cleanup skipped because the configured failure state is not startable."
		);

		return Ok(());
	}

	let Some(state_id) = context.issue_run.issue.state_id_for_name(failure_state_name) else {
		tracing::warn!(
			issue_id = context.issue_run.issue.id,
			issue = context.issue_run.issue.identifier,
			target_state = failure_state_name,
			"Retryable failed-start cleanup skipped because the target state id was not available."
		);

		return Ok(());
	};

	context.tracker.update_issue_state(&context.issue_run.issue.id, state_id)?;

	ensure_automation_activity_label(
		context.tracker,
		&context.issue_run.issue,
		context.project.service_id(),
		false,
	)?;

	context.state_store.clear_worktree(&context.issue_run.issue.id)?;
	context
		.state_store
		.append_private_execution_event(
			context.project.service_id(),
			&context.issue_run.issue.id,
			&context.issue_run.run_id,
			context.issue_run.attempt_number,
			RETRYABLE_FAILED_START_CLEANUP_EVENT_TYPE,
			json!({
				"schema": "decodex.retryable_failed_start_cleanup/1",
				"source_error_class": retained_progress_source_error_class(error)
					.unwrap_or("retryable_execution_failure"),
				"dispatch_mode": context.issue_run.dispatch_mode.as_str(),
				"active_label_cleared": true,
				"worktree_mapping_cleared": true,
				"target_issue_state": failure_state_name,
				"issue_state_reset": true,
				"retryable_by_next_program_pass": true,
			}),
		)
		.map(|_| ())?;

	tracing::info!(
		project_id = context.project.service_id(),
		issue_id = context.issue_run.issue.id,
		issue = context.issue_run.issue.identifier,
		run_id = context.issue_run.run_id,
		attempt = context.issue_run.attempt_number,
		branch = context.issue_run.worktree.branch_name,
		worktree_path = %context.worktree_path,
		issue_state_reset = true,
		"Cleared retryable failed-start ownership after a no-diff Program run failure."
	);

	Ok(())
}

fn retryable_failed_start_cleanup_allowed<T>(
	context: &FailureHandlingContext<'_, T>,
	error: &Report,
) -> Result<bool>
where
	T: IssueTracker,
{
	if context.issue_run.dispatch_mode != IssueDispatchMode::Program {
		return Ok(false);
	}
	if !retryable_failure_happened_before_effective_agent_execution(error) {
		return Ok(false);
	}
	if context.state_store.lease_for_issue(&context.issue_run.issue.id)?.is_some() {
		return Ok(false);
	}
	if context.state_store.issue_has_review_lifecycle_record(
		context.project.service_id(),
		&context.issue_run.issue.id,
	)? {
		return Ok(false);
	}
	if latest_open_issue_phase_goal_before_attempt(
		context.project,
		context.state_store,
		&context.issue_run.issue.id,
		&context.issue_run.run_id,
		context.issue_run.attempt_number,
	)?
	.is_some()
	{
		return Ok(false);
	}

	Ok(loop_guardrail_worktree_fingerprint(&context.issue_run.worktree.path)?
		.is_some_and(|fingerprint| !fingerprint.effective_delta_present))
}

fn retryable_failure_happened_before_effective_agent_execution(error: &Report) -> bool {
	error.downcast_ref::<AppServerZeroEvidenceStartFailure>().is_some()
		|| error
			.downcast_ref::<AppServerCapabilityPreflightFailure>()
			.is_some_and(AppServerCapabilityPreflightFailure::is_retryable_timeout)
		|| error
			.downcast_ref::<AppServerTransportFailure>()
			.is_some_and(AppServerTransportFailure::is_retryable_startup)
}

fn retryable_failure_validation_result(
	error: &Report,
	retry_error_class: &str,
) -> Option<&'static str> {
	if retry_error_class.starts_with("repo_gate_")
		|| error.downcast_ref::<RepoGateFailure>().is_some()
	{
		Some("failed")
	} else {
		None
	}
}

fn apply_architecture_recovery_retry_writeback<T>(
	context: &FailureHandlingContext<'_, T>,
	recovery: ArchitectureRecoveryStart,
	max_attempts: i64,
) -> Result<()>
where
	T: IssueTracker,
{
	let retry_attempt = u32::try_from(context.retry_budget_attempts).unwrap_or(u32::MAX).max(1);
	let delay = retry_delay(RetryKind::Failure, retry_attempt, context.workflow);
	let retry_ready_at_unix_epoch = OffsetDateTime::now_utc().unix_timestamp().saturating_add(
		i64::try_from((delay.as_millis().saturating_add(999)) / 1_000).unwrap_or(i64::MAX),
	);
	let recovery_max_attempts =
		max_attempts.saturating_add(i64::try_from(recovery.max_attempts).unwrap_or(0));

	state::write_run_retry_schedule(
		&context.issue_run.worktree.path,
		&context.issue_run.run_id,
		context.issue_run.attempt_number,
		ARCHITECTURE_RECOVERY_RETRY_KIND,
		retry_ready_at_unix_epoch,
	)?;

	write_retry_budget_marker(
		&context.issue_run.worktree.path,
		&context.issue_run.run_id,
		context.issue_run.attempt_number,
		context.retry_budget_attempts,
	)?;

	tracing::warn!(
		project_id = context.project.service_id(),
		issue_id = context.issue_run.issue.id,
		issue = context.issue_run.issue.identifier,
		run_id = context.issue_run.run_id,
		attempt = context.issue_run.attempt_number,
		recovery_attempt = recovery.attempt_number,
		max_recovery_attempts = recovery.max_attempts,
		branch = context.issue_run.worktree.branch_name,
		worktree_path = %context.worktree_path,
		"Loop guardrail started autonomous architecture recovery."
	);

	tracker::create_public_comment(
		context.tracker,
		&context.issue_run.issue.id,
		&format_retry_comment(RetryComment {
			run_id: &context.issue_run.run_id,
			attempt_number: context.issue_run.attempt_number,
			retry_budget_attempt_number: context.retry_budget_attempts,
			max_attempts: recovery_max_attempts,
			worktree_path: context.worktree_path.to_owned(),
			branch_name: &context.issue_run.worktree.branch_name,
			error_class: "architecture_recovery_started",
			next_action: architecture_recovery_retry_next_action(recovery.policy_decision),
		}),
	)?;

	record_harness_outcome_best_effort(
		context.state_store,
		context.project.service_id(),
		context.issue_run,
		HarnessOutcomeKind::RetryableFailure,
		Some("architecture_recovery_started"),
		Some("architecture_recovery_started"),
		None,
	);

	Ok(())
}

fn apply_loop_guardrail_failure_writeback<T>(
	context: &FailureHandlingContext<'_, T>,
	loop_guardrail_stop: LoopGuardrailStopRequested,
) -> Result<()>
where
	T: IssueTracker,
{
	let terminal_error = Report::new(loop_guardrail_stop);
	let privacy_classifier = configured_public_projection_privacy_classifier(context.project)?;
	let outcome = apply_terminal_failure_writeback(
		context.tracker,
		TerminalFailureWritebackRuntime {
			service_id: context.project.service_id(),
			state_store: Some(context.state_store),
			privacy_classifier: &privacy_classifier,
		},
		context.workflow,
		context.issue_run,
		context.worktree_path,
		false,
		&terminal_error,
	)?;

	if outcome.retry_guarded_by_state {
		write_terminal_guard_marker(
			&context.issue_run.worktree.path,
			&context.issue_run.run_id,
			context.issue_run.attempt_number,
		)?;

		context
			.state_store
			.update_run_status(&context.issue_run.run_id, TERMINAL_GUARDED_RUN_STATUS)?;
	}

	write_retry_budget_marker(
		&context.issue_run.worktree.path,
		&context.issue_run.run_id,
		context.issue_run.attempt_number,
		context.retry_budget_attempts,
	)?;

	tracing::warn!(
		project_id = context.project.service_id(),
		issue_id = context.issue_run.issue.id,
		issue = context.issue_run.issue.identifier,
		run_id = context.issue_run.run_id,
		attempt = context.issue_run.attempt_number,
		branch = context.issue_run.worktree.branch_name,
		worktree_path = context.worktree_path,
		error_class = outcome.error_class,
		"Run stopped by loop guardrail."
	);

	Ok(())
}

pub(super) fn retry_budget_attempts_for_current_failure(
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
) -> Result<i64> {
	let state_attempts = state_store.retry_budget_attempt_count(&issue_run.issue.id)?;
	let current_attempt_counts =
		state_store.run_attempt(&issue_run.run_id)?.is_some_and(|attempt| {
			attempt.issue_id() == issue_run.issue.id
				&& matches!(attempt.status(), "failed" | "interrupted" | "terminal_guarded")
		});
	let previous_state_attempts = state_attempts.saturating_sub(i64::from(current_attempt_counts));

	Ok(issue_run.retry_budget_base.max(previous_state_attempts) + i64::from(current_attempt_counts))
}

fn retained_partial_progress_error(
	error: &Report,
	issue_run: &IssueRunPlan,
	worktree_path: &str,
) -> Option<Report> {
	if retained_progress_should_defer_to_terminal_intent(error)
		|| !worktree_has_tracked_changes(&issue_run.worktree.path)
	{
		return None;
	}

	Some(Report::new(RetainedPartialProgress {
		issue_identifier: issue_run.issue.identifier.clone(),
		run_id: issue_run.run_id.clone(),
		worktree_path: worktree_path.to_owned(),
		source_error_class: retained_progress_source_error_class(error).map(ToOwned::to_owned),
	}))
}

fn retained_progress_should_defer_to_terminal_intent(error: &Report) -> bool {
	error.downcast_ref::<ManualAttentionRequested>().is_some()
		|| error.downcast_ref::<LoopGuardrailStopRequested>().is_some()
		|| error.downcast_ref::<ReviewHandoffNeedsAttention>().is_some()
		|| error.downcast_ref::<RetainedReviewRepairPushFailed>().is_some()
		|| error.downcast_ref::<RetainedPartialProgress>().is_some()
		|| error.downcast_ref::<RetainedReviewNeedsAttention>().is_some()
		|| error.downcast_ref::<ReviewPolicyStopRequested>().is_some()
		|| error.downcast_ref::<CodexAccountAuthFailure>().is_some()
}

pub(super) fn retained_progress_source_error_class(error: &Report) -> Option<&'static str> {
	if let Some(app_server_failure) = error.downcast_ref::<AppServerZeroEvidenceStartFailure>() {
		Some(app_server_failure.error_class())
	} else if error.downcast_ref::<StalledRunNeedsAttention>().is_some() {
		Some("stalled_run_detected")
	} else if error.downcast_ref::<AgentGitCredentialsUnavailable>().is_some() {
		Some("github_credentials_unavailable")
	} else if let Some(push_failure) = error.downcast_ref::<RetainedReviewRepairPushFailed>() {
		Some(push_failure.error_class())
	} else if let Some(app_server_failure) =
		error.downcast_ref::<AppServerCapabilityPreflightFailure>()
	{
		Some(app_server_failure.error_class())
	} else if let Some(app_server_failure) = error.downcast_ref::<AppServerHomePreflightFailure>() {
		Some(app_server_failure.error_class())
	} else if let Some(account_failure) = error.downcast_ref::<CodexAccountAuthFailure>() {
		Some(account_failure.error_class())
	} else if let Some(app_server_failure) = error.downcast_ref::<AppServerTransportFailure>() {
		Some(app_server_failure.error_class())
	} else if let Some(app_server_failure) = error.downcast_ref::<AppServerPhaseGoalFailure>() {
		Some(app_server_failure.error_class())
	} else if let Some(app_server_failure) = error.downcast_ref::<AppServerDynamicToolFailure>() {
		Some(app_server_failure.error_class())
	} else if let Some(app_server_failure) = error.downcast_ref::<AppServerTurnFailure>() {
		Some(app_server_failure.error_class())
	} else if let Some(repo_gate_failure) = error.downcast_ref::<RepoGateFailure>() {
		Some(repo_gate_failure.error_class())
	} else if let Some(acceptance_failure) = error.downcast_ref::<PhaseAcceptanceCheckFailure>() {
		Some(acceptance_failure.error_class())
	} else {
		None
	}
}

pub(super) fn write_retry_schedule_marker_for_runtime_retry(
	error: &Report,
	workflow: &WorkflowDocument,
	issue_run: &IssueRunPlan,
	retry_budget_attempts: i64,
) -> Result<()> {
	if error.downcast_ref::<StalledRunNeedsAttention>().is_some() {
		return write_failure_retry_schedule_marker(workflow, issue_run, retry_budget_attempts);
	}
	if error
		.downcast_ref::<AppServerCapabilityPreflightFailure>()
		.is_some_and(AppServerCapabilityPreflightFailure::is_retryable_timeout)
	{
		return write_failure_retry_schedule_marker(workflow, issue_run, retry_budget_attempts);
	}
	if error
		.downcast_ref::<AppServerPhaseGoalFailure>()
		.is_some_and(AppServerPhaseGoalFailure::is_terminal_path_missing)
	{
		return write_failure_retry_schedule_marker(workflow, issue_run, retry_budget_attempts);
	}

	let Some(repo_gate_failure) = error.downcast_ref::<RepoGateFailure>() else {
		return Ok(());
	};
	let Some(retry_kind) = repo_gate_failure.retry_schedule_kind() else {
		return Ok(());
	};

	write_retry_schedule_marker(workflow, issue_run, retry_budget_attempts, retry_kind)
}

fn write_failure_retry_schedule_marker(
	workflow: &WorkflowDocument,
	issue_run: &IssueRunPlan,
	retry_budget_attempts: i64,
) -> Result<()> {
	write_retry_schedule_marker(workflow, issue_run, retry_budget_attempts, "failure")
}

fn write_retry_schedule_marker(
	workflow: &WorkflowDocument,
	issue_run: &IssueRunPlan,
	retry_budget_attempts: i64,
	retry_kind: &str,
) -> Result<()> {
	let retry_attempt = u32::try_from(retry_budget_attempts).unwrap_or(u32::MAX).max(1);
	let delay = retry_delay(RetryKind::Failure, retry_attempt, workflow);
	let retry_ready_at_unix_epoch = OffsetDateTime::now_utc().unix_timestamp().saturating_add(
		i64::try_from((delay.as_millis().saturating_add(999)) / 1_000).unwrap_or(i64::MAX),
	);

	state::write_run_retry_schedule(
		&issue_run.worktree.path,
		&issue_run.run_id,
		issue_run.attempt_number,
		retry_kind,
		retry_ready_at_unix_epoch,
	)
}

pub(super) fn ensure_automation_activity_label<T>(
	tracker: &T,
	issue: &TrackerIssue,
	service_id: &str,
	present: bool,
) -> Result<()>
where
	T: IssueTracker,
{
	let mut refreshed_issues = tracker.refresh_issues(slice::from_ref(&issue.id))?;
	let current_issue = refreshed_issues.pop().unwrap_or_else(|| issue.clone());
	let active_label = tracker::automation_active_label(service_id);

	tracker::set_issue_label_presence(tracker, &current_issue, &active_label, present)?;

	Ok(())
}
