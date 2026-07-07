use crate::{
	orchestrator::execution_failure::{
		self, IssueRunPlan, IssueTracker, LoopGuardrailReason, LoopGuardrailWorktreeFingerprint,
		ManualAttentionRequested, Report, Result, ServiceConfig, StateStore,
		TERMINAL_GUARDED_RUN_STATUS, TerminalFailureWritebackRuntime, WorkflowDocument,
		review_handoff_drift::{
			lineage, recovery::transition, types::REVIEW_HANDOFF_STATE_DRIFT_DETECTED_EVENT_TYPE,
		},
	},
	state::ReviewLifecycleRecord,
};

pub(super) fn review_handoff_state_drift_attention_error(
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	error: &Report,
) -> Result<Option<ManualAttentionRequested>> {
	if !lineage::review_handoff_failure_drift_can_handle(error) {
		return Ok(None);
	}

	let Some(worktree_fingerprint) =
		execution_failure::loop_guardrail_worktree_fingerprint(&issue_run.worktree.path)?
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
	let drift_reason = match state_store.review_lifecycle_record(
		project.service_id(),
		&issue_run.issue.id,
		&issue_run.worktree.branch_name,
	)? {
		Some(lifecycle_record) => review_lifecycle_record_drift_reason(
			workflow,
			issue_run,
			&worktree_fingerprint,
			&lifecycle_record,
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

			Some(String::from("missing_review_lifecycle_record"))
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
			serde_json::json!({
				"schema": "decodex.review_handoff_state_drift_detected/1",
				"reason": drift_reason,
				"source_error_class": lineage::review_handoff_failure_drift_source_error_class(error),
				"branch_name": issue_run.worktree.branch_name,
				"checkpoint_status": checkpoint.as_ref().map(|checkpoint| checkpoint.status()),
				"checkpoint_head_sha": checkpoint.as_ref().map(|checkpoint| checkpoint.head_sha()),
				"local_head_sha": worktree_fingerprint.head_sha,
				"next_action": "restore or rebind the retained review lifecycle authority before retrying execution",
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

pub(super) fn apply_review_handoff_state_drift_attention_writeback<T>(
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
	let privacy_classifier =
		execution_failure::configured_public_projection_privacy_classifier(project)?;
	let outcome = execution_failure::apply_terminal_failure_writeback(
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
		execution_failure::write_terminal_guard_marker(
			&issue_run.worktree.path,
			&issue_run.run_id,
			issue_run.attempt_number,
		)?;

		state_store.update_run_status(&issue_run.run_id, TERMINAL_GUARDED_RUN_STATUS)?;
	}

	Ok(())
}

fn review_lifecycle_record_drift_reason(
	workflow: &WorkflowDocument,
	issue_run: &IssueRunPlan,
	worktree_fingerprint: &LoopGuardrailWorktreeFingerprint,
	lifecycle_record: &ReviewLifecycleRecord,
) -> Result<Option<String>> {
	if lifecycle_record.branch_name() != issue_run.worktree.branch_name {
		return Ok(Some(String::from("review_lifecycle_authority_branch_mismatch")));
	}
	if lifecycle_record.pr_head_ref_name() != issue_run.worktree.branch_name {
		return Ok(Some(String::from("review_lifecycle_authority_pr_head_ref_mismatch")));
	}

	let lineage = lineage::review_handoff_failure_drift_lineage(
		&issue_run.worktree.path,
		lifecycle_record.pr_head_oid(),
		&worktree_fingerprint.head_sha,
	);

	if !lineage.allows_lifecycle_recovery() {
		return Ok(Some(format!("review_lifecycle_authority_{}", lineage.as_str())));
	}
	if transition::review_handoff_state_drift_success_transition(workflow, issue_run)?.is_some() {
		return Ok(None);
	}

	Ok(Some(String::from("review_lifecycle_authority_issue_state_unsupported")))
}
