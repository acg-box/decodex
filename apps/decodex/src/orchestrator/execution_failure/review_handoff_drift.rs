use super::{
	CodexAccountAuthFailure, Command, IssueRunPlan, IssueTracker, LoopGuardrailReason,
	LoopGuardrailStopRequested, LoopGuardrailWorktreeFingerprint, ManualAttentionRequested, Path,
	Report, Result, RetainedReviewNeedsAttention, ReviewHandoffMarker, ReviewHandoffNeedsAttention,
	ReviewOrchestrationMarker, ReviewPolicyStopRequested, ServiceConfig, StateStore,
	TERMINAL_GUARDED_RUN_STATUS, TerminalFailureWritebackRuntime, WorkflowDocument,
	apply_terminal_failure_writeback, configured_public_projection_privacy_classifier, eyre, json,
	loop_guardrail_worktree_fingerprint, retained_progress_source_error_class,
	run_failure_requires_terminal_attention, tracker, write_terminal_guard_marker,
};

const REVIEW_HANDOFF_STATE_DRIFT_DETECTED_EVENT_TYPE: &str = "review_handoff_state_drift_detected";
const REVIEW_HANDOFF_STATE_DRIFT_RECOVERED_EVENT_TYPE: &str =
	"review_handoff_state_drift_recovered";
const REVIEW_HANDOFF_REBOUND_ORCHESTRATION_PHASE: &str = "request_pending";

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

pub(super) fn handle_review_handoff_failure_drift<T>(
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

fn try_recover_review_handoff_failure_drift<T>(
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
