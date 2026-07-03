use color_eyre::eyre;

use crate::{
	orchestrator::{
		execution_failure::{
			self, IssueRunPlan, IssueTracker, Report, Result, ReviewHandoffMarker,
			ReviewOrchestrationMarker, ServiceConfig, StateStore, WorkflowDocument,
			review_handoff_drift::{
				attention, command, lineage,
				types::{
					REVIEW_HANDOFF_REBOUND_ORCHESTRATION_PHASE,
					REVIEW_HANDOFF_STATE_DRIFT_RECOVERED_EVENT_TYPE,
					ReviewHandoffStateDriftTransition,
				},
			},
		},
		kernel::command::CommandIntentKind,
	},
	tracker,
};

pub(crate) fn handle_review_handoff_failure_drift<T>(
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

	let Some(attention_error) = attention::review_handoff_state_drift_attention_error(
		project,
		workflow,
		state_store,
		issue_run,
		error,
	)?
	else {
		return Ok(false);
	};

	attention::apply_review_handoff_state_drift_attention_writeback(
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

pub(super) fn review_handoff_state_drift_success_transition(
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
	if !lineage::review_handoff_failure_drift_can_handle(error) {
		return Ok(false);
	}

	let Some(worktree_fingerprint) =
		execution_failure::loop_guardrail_worktree_fingerprint(&issue_run.worktree.path)?
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

	let lineage = lineage::review_handoff_failure_drift_lineage(
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
			serde_json::json!({
				"schema": "decodex.review_handoff_state_drift_recovered/1",
				"reason": "current_review_handoff_marker",
				"source_error_class": lineage::review_handoff_failure_drift_source_error_class(error),
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

	command::review_handoff_drift_command_adapter(
		command::review_handoff_drift_marker_rebind_command_intent(
			&issue_run.issue.id,
			review_handoff.run_id(),
		),
		CommandIntentKind::SyncReviewOrchestrationMarker,
	)?;

	state_store.upsert_review_orchestration_marker(
		project.service_id(),
		&issue_run.issue.id,
		&orchestration_marker,
	)?;

	Ok(rebounded_orchestration)
}
