use crate::orchestrator::execution_failure::{
	self, IssueRunPlan, LOOP_GUARDRAIL_CONVERGENCE_BUDGET, LoopGuardrailCheckpointInput,
	LoopGuardrailReason, LoopGuardrailStopRequested, RepoGateFailure, RepoGateFailureDiagnostic,
	RepoGateFailureDisposition, Report, Result, ServiceConfig, StateStore,
	loop_guardrail::{event, fingerprint},
};

pub(crate) fn retryable_failure_loop_guardrail_stop(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	error: &Report,
) -> Result<Option<LoopGuardrailStopRequested>> {
	let Some(worktree_fingerprint) =
		fingerprint::loop_guardrail_worktree_fingerprint(&issue_run.worktree.path)?
	else {
		return Ok(None);
	};
	let repo_gate_diagnostic = error
		.downcast_ref::<RepoGateFailure>()
		.and_then(RepoGateFailure::diagnostic)
		.map(RepoGateFailureDiagnostic::to_json);
	let mut observations = Vec::new();

	if let Some(repo_gate_failure) = error.downcast_ref::<RepoGateFailure>()
		&& repo_gate_failure.disposition() == RepoGateFailureDisposition::ContinueRepair
	{
		observations.push((
			LoopGuardrailReason::ValidationRepeat,
			loop_guardrail_normalized_validation_fingerprint(repo_gate_failure.error_class()),
			Some(repo_gate_failure.error_class()),
		));
		observations.push((
			LoopGuardrailReason::RemainingDeltaUnchanged,
			format!(
				"{}:{}:{}:{}",
				repo_gate_failure.error_class(),
				worktree_fingerprint.head_sha.as_str(),
				worktree_fingerprint.effective_status_hash.as_str(),
				worktree_fingerprint.tracked_diff_hash.as_str()
			),
			Some(repo_gate_failure.error_class()),
		));
	}

	if !worktree_fingerprint.effective_delta_present {
		observations.push((
			LoopGuardrailReason::NoEffectiveDiff,
			format!(
				"{}:{}",
				worktree_fingerprint.effective_status_hash.as_str(),
				worktree_fingerprint.tracked_diff_hash.as_str()
			),
			execution_failure::retained_progress_source_error_class(error),
		));
	}

	for (reason, fingerprint, source_error_class) in observations {
		let mut details = execution_failure::json!({
			"schema": "decodex.loop_guardrail_checkpoint/1",
			"reason": reason.error_class(),
			"source_error_class": source_error_class,
			"head_sha": worktree_fingerprint.head_sha.as_str(),
			"tracked_status_hash": worktree_fingerprint.tracked_status_hash.as_str(),
			"tracked_diff_hash": worktree_fingerprint.tracked_diff_hash.as_str(),
			"effective_status_hash": worktree_fingerprint.effective_status_hash.as_str(),
			"branch_delta_present": worktree_fingerprint.branch_delta_present,
			"effective_delta_present": worktree_fingerprint.effective_delta_present,
			"threshold": LOOP_GUARDRAIL_CONVERGENCE_BUDGET,
		});

		if let Some(diagnostic) = &repo_gate_diagnostic {
			details["repo_gate_failure"] = diagnostic.clone();
		}

		let details_json = details.to_string();
		let checkpoint =
			state_store.observe_loop_guardrail_checkpoint(LoopGuardrailCheckpointInput {
				project_id: project.service_id(),
				issue_id: &issue_run.issue.id,
				reason: reason.error_class(),
				fingerprint: &fingerprint,
				run_id: &issue_run.run_id,
				attempt_number: issue_run.attempt_number,
				details_json: &details_json,
			})?;

		event::record_loop_guardrail_private_event(
			project,
			state_store,
			issue_run,
			&checkpoint,
			source_error_class,
		)?;

		if checkpoint.consecutive_count() >= LOOP_GUARDRAIL_CONVERGENCE_BUDGET {
			return Ok(Some(LoopGuardrailStopRequested {
				issue_identifier: issue_run.issue.identifier.clone(),
				run_id: issue_run.run_id.clone(),
				reason,
				consecutive_count: checkpoint.consecutive_count(),
				fingerprint,
				source_error_class: source_error_class.map(ToOwned::to_owned),
				architecture_recovery_reason_code: None,
			}));
		}
	}

	Ok(None)
}

fn loop_guardrail_normalized_validation_fingerprint(error_class: &str) -> String {
	format!("{error_class}:repo_gate:validation_repair:lane_authority")
}
