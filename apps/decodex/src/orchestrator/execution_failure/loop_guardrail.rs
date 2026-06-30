use super::{
	Command, Digest, IssueRunPlan, LOOP_GUARDRAIL_CONVERGENCE_BUDGET, LoopGuardrailCheckpoint,
	LoopGuardrailCheckpointInput, LoopGuardrailReason, LoopGuardrailStopRequested,
	LoopGuardrailWorktreeFingerprint, Path, RepoGateFailure, RepoGateFailureDiagnostic,
	RepoGateFailureDisposition, Report, Result, ReviewPolicyStopRequested, ServiceConfig, Sha256,
	StateStore, json, repo_gate_changed_tracked_files, retained_progress_source_error_class,
	run_failure_writeback_disposition, state, worktree_head_oid,
};

pub(in crate::orchestrator) fn retryable_failure_loop_guardrail_stop(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	error: &Report,
) -> Result<Option<LoopGuardrailStopRequested>> {
	let Some(worktree_fingerprint) = loop_guardrail_worktree_fingerprint(&issue_run.worktree.path)?
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
			format!(
				"{}:{}:{}",
				repo_gate_failure.error_class(),
				worktree_fingerprint.head_sha.as_str(),
				loop_guardrail_text_hash(&error.to_string())
			),
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
			retained_progress_source_error_class(error),
		));
	}

	for (reason, fingerprint, source_error_class) in observations {
		let mut details = json!({
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

		record_loop_guardrail_private_event(
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

pub(in crate::orchestrator) fn loop_guardrail_worktree_fingerprint(
	worktree_path: &Path,
) -> Result<Option<LoopGuardrailWorktreeFingerprint>> {
	let Some(head_sha) = worktree_head_oid(worktree_path)? else {
		return Ok(None);
	};
	let Some(tracked_status) =
		git_guardrail_output(worktree_path, &["status", "--porcelain", "--untracked-files=no"])?
	else {
		return Ok(None);
	};
	let Some(raw_status) = git_guardrail_output(worktree_path, &["status", "--porcelain"])? else {
		return Ok(None);
	};
	let Some(tracked_diff) =
		git_guardrail_output(worktree_path, &["diff", "--binary", "--no-ext-diff", "HEAD", "--"])?
	else {
		return Ok(None);
	};
	let effective_status = loop_guardrail_effective_status(&raw_status);
	let branch_delta_present = repo_gate_changed_tracked_files(worktree_path)
		.is_ok_and(|changed_files| !changed_files.is_empty());

	Ok(Some(LoopGuardrailWorktreeFingerprint {
		head_sha,
		tracked_status_hash: loop_guardrail_text_hash(&tracked_status),
		tracked_diff_hash: loop_guardrail_text_hash(&tracked_diff),
		effective_status_hash: loop_guardrail_text_hash(&effective_status),
		branch_delta_present,
		effective_delta_present: branch_delta_present
			|| !effective_status.trim().is_empty()
			|| !tracked_diff.trim().is_empty(),
	}))
}

pub(in crate::orchestrator) fn loop_guardrail_effective_status(raw_status: &str) -> String {
	let lines = raw_status
		.lines()
		.map(str::trim_end)
		.filter(|line| !line.is_empty())
		.filter(|line| !state::is_untracked_decodex_runtime_artifact_status_line(line))
		.collect::<Vec<_>>();

	if lines.is_empty() {
		return String::new();
	}

	let mut status = lines.join("\n");

	status.push('\n');

	status
}

pub(in crate::orchestrator) fn git_guardrail_output(
	worktree_path: &Path,
	args: &[&str],
) -> Result<Option<String>> {
	let output = Command::new("git").arg("-C").arg(worktree_path).args(args).output()?;

	if !output.status.success() {
		return Ok(None);
	}

	Ok(Some(String::from_utf8_lossy(&output.stdout).into_owned()))
}

pub(in crate::orchestrator) fn loop_guardrail_text_hash(text: &str) -> String {
	let digest = <Sha256 as Digest>::digest(text.as_bytes());
	let mut hash = String::with_capacity(64);

	for byte in digest {
		hash.push(char::from(b"0123456789abcdef"[(byte >> 4) as usize]));
		hash.push(char::from(b"0123456789abcdef"[(byte & 0x0f) as usize]));
	}

	hash
}

fn record_loop_guardrail_private_event(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	checkpoint: &LoopGuardrailCheckpoint,
	source_error_class: Option<&str>,
) -> Result<()> {
	state_store
		.append_private_execution_event(
			project.service_id(),
			&issue_run.issue.id,
			&issue_run.run_id,
			issue_run.attempt_number,
			"loop_guardrail_checkpoint",
			json!({
				"schema": "decodex.loop_guardrail_checkpoint/1",
				"reason": checkpoint.reason(),
				"fingerprint": checkpoint.fingerprint(),
				"consecutive_count": checkpoint.consecutive_count(),
				"threshold": LOOP_GUARDRAIL_CONVERGENCE_BUDGET,
				"checkpoint_run_id": checkpoint.run_id(),
				"checkpoint_attempt_number": checkpoint.attempt_number(),
				"source_error_class": source_error_class,
				"details": checkpoint.details_json(),
			}),
		)
		.map(|_| ())
}

pub(in crate::orchestrator) fn loop_guardrail_stop_from_review_policy(
	review_policy_stop: &ReviewPolicyStopRequested,
) -> LoopGuardrailStopRequested {
	LoopGuardrailStopRequested {
		issue_identifier: review_policy_stop.issue_identifier.clone(),
		run_id: review_policy_stop.run_id.clone(),
		reason: LoopGuardrailReason::ReviewChurn,
		consecutive_count: review_policy_stop.nonclean_rounds.unwrap_or_default(),
		fingerprint: review_policy_stop.fingerprint.clone().unwrap_or_else(|| {
			format!(
				"{}:{}",
				review_policy_stop.head_sha,
				review_policy_stop.nonclean_rounds.unwrap_or_default()
			)
		}),
		source_error_class: Some(review_policy_stop.reason.error_class().to_owned()),
		architecture_recovery_reason_code: None,
	}
}

pub(in crate::orchestrator) fn run_failure_requires_terminal_attention(error: &Report) -> bool {
	run_failure_writeback_disposition(error).requires_terminal_attention()
}
