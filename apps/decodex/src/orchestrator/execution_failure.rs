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
	architecture_recovery_retry_next_action, configured_public_projection_privacy_classifier, eyre,
	format_retry_comment, format_terminal_failure_comment, json,
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
mod retryable_writeback;
mod review_handoff_drift;
mod terminal_writeback;
mod zero_evidence;

pub(super) use loop_guardrail::{
	git_guardrail_output, loop_guardrail_effective_status, loop_guardrail_stop_from_review_policy,
	loop_guardrail_text_hash, loop_guardrail_worktree_fingerprint,
	retryable_failure_loop_guardrail_stop, run_failure_requires_terminal_attention,
};
use retryable_writeback::apply_retryable_failure_writeback;
#[cfg(test)] pub(super) use retryable_writeback::write_retry_schedule_marker_for_runtime_retry;
use review_handoff_drift::handle_review_handoff_failure_drift;
pub(super) use terminal_writeback::apply_terminal_failure_writeback;
pub(super) use zero_evidence::{
	AppServerZeroEvidenceStartFailure, promote_zero_evidence_app_server_start_failure,
	truncate_private_diagnostic_text,
};

pub(super) const LOOP_GUARDRAIL_CONVERGENCE_BUDGET: i64 = 3;
pub(super) const ARCHITECTURE_RECOVERY_BUDGET: usize = 1;
pub(super) const ARCHITECTURE_RECOVERY_RETRY_KIND: &str = "architecture_recovery";
pub(super) const RETRYABLE_FAILED_START_CLEANUP_EVENT_TYPE: &str = "retryable_failed_start_cleanup";

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
			LoopGuardrailRecoveryDecision::Start(recovery) =>
				apply_architecture_recovery_retry_writeback(
					&failure_context,
					recovery,
					max_attempts,
				),
			LoopGuardrailRecoveryDecision::HumanRequired(loop_guardrail_stop) =>
				apply_loop_guardrail_failure_writeback(&failure_context, loop_guardrail_stop),
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
			LoopGuardrailRecoveryDecision::Start(recovery) =>
				apply_architecture_recovery_retry_writeback(
					&failure_context,
					recovery,
					max_attempts,
				),
			LoopGuardrailRecoveryDecision::HumanRequired(loop_guardrail_stop) =>
				apply_loop_guardrail_failure_writeback(&failure_context, loop_guardrail_stop),
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
