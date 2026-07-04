use serde_json::json;

use crate::orchestrator::execution_architecture_recovery::{
	self, DecisionContractRecord, ExecutionProgramRecord, IssueRunPlan, LoopGuardrailStopRequested,
	Path, Report, Result, ServiceConfig, StateStore, Value, loop_guardrail_effective_status,
	truncate_private_diagnostic_text,
};

pub(super) fn architecture_recovery_programs_for_contracts(
	state_store: &StateStore,
	project_id: &str,
	contracts: &[DecisionContractRecord],
) -> Result<Vec<ExecutionProgramRecord>> {
	let mut programs = Vec::new();

	for contract in contracts {
		for program in
			state_store.list_execution_programs_for_contract(project_id, contract.contract_id())?
		{
			if programs.iter().all(|existing: &ExecutionProgramRecord| {
				existing.program_id() != program.program_id()
			}) {
				programs.push(program);
			}
		}
	}

	programs.sort_by(|left, right| left.program_id().cmp(right.program_id()));

	Ok(programs)
}

pub(super) fn architecture_recovery_retained_worktree(worktree_path: &Path) -> Result<Value> {
	let fingerprint =
		execution_architecture_recovery::loop_guardrail_worktree_fingerprint(worktree_path)?;
	let tracked_status = execution_architecture_recovery::git_guardrail_output(
		worktree_path,
		&["status", "--porcelain", "--untracked-files=no"],
	)?;
	let raw_status = execution_architecture_recovery::git_guardrail_output(
		worktree_path,
		&["status", "--porcelain"],
	)?;
	let effective_status = raw_status.as_deref().map(loop_guardrail_effective_status);
	let diff_stat = execution_architecture_recovery::git_guardrail_output(
		worktree_path,
		&["diff", "--stat", "--no-ext-diff", "HEAD", "--"],
	)?;

	Ok(execution_architecture_recovery::json!({
		"head_sha": fingerprint.as_ref().map(|value| value.head_sha.as_str()),
		"tracked_status_hash": fingerprint
			.as_ref()
			.map(|value| value.tracked_status_hash.as_str()),
		"tracked_diff_hash": fingerprint.as_ref().map(|value| value.tracked_diff_hash.as_str()),
		"effective_status_hash": fingerprint
			.as_ref()
			.map(|value| value.effective_status_hash.as_str()),
		"effective_delta_present": fingerprint
			.as_ref()
			.map(|value| value.effective_delta_present),
		"tracked_status": tracked_status.unwrap_or_default(),
		"effective_status": effective_status.unwrap_or_default(),
		"diff_stat": diff_stat.unwrap_or_default(),
	}))
}

pub(super) fn architecture_recovery_review_findings(
	state_store: &StateStore,
	project: &ServiceConfig,
	issue_run: &IssueRunPlan,
) -> Result<Value> {
	let events = state_store
		.list_private_execution_events_for_issue(project.service_id(), &issue_run.issue.id)?;
	let latest_review = events
		.iter()
		.rev()
		.find(|event| event.event_type() == "review_checkpoint")
		.map(|event| event.payload());
	let Some(payload) = latest_review else {
		return Ok(execution_architecture_recovery::json!({
			"latest_status": null,
			"accepted_finding_count": 0,
			"rejected_finding_count": 0,
		}));
	};
	let review = payload.get("review").unwrap_or(payload);
	let route_summary = review.get("finding_route_summary");

	Ok(execution_architecture_recovery::json!({
		"latest_status": payload.get("status").and_then(Value::as_str),
		"accepted_finding_count": review
			.get("accepted_findings")
			.and_then(Value::as_array)
			.map_or(0, Vec::len),
		"rejected_finding_count": review
			.get("rejected_findings")
			.and_then(Value::as_array)
			.map_or(0, Vec::len),
		"route_counts": route_summary
			.and_then(|summary| summary.get("route_counts"))
			.cloned()
			.unwrap_or_else(|| json!([])),
		"route_next_action": route_summary
			.and_then(|summary| summary.get("next_action"))
			.and_then(Value::as_str),
		"nonclean_rounds": payload.get("nonclean_rounds").and_then(Value::as_i64).unwrap_or(0),
	}))
}

pub(super) fn architecture_recovery_issue_payload(issue_run: &IssueRunPlan) -> Value {
	execution_architecture_recovery::json!({
		"id": issue_run.issue.id.as_str(),
		"identifier": issue_run.issue.identifier.as_str(),
		"title": issue_run.issue.title.as_str(),
	})
}

pub(super) fn architecture_recovery_run_payload(issue_run: &IssueRunPlan) -> Value {
	execution_architecture_recovery::json!({
		"run_id": issue_run.run_id.as_str(),
		"attempt_number": issue_run.attempt_number,
		"branch": issue_run.worktree.branch_name.as_str(),
		"dispatch_mode": issue_run.dispatch_mode.as_str(),
	})
}

pub(super) fn architecture_recovery_contract_payload(record: &DecisionContractRecord) -> Value {
	execution_architecture_recovery::json!({
		"contract_id": record.contract_id(),
		"source_issue_id": record.source_issue_id(),
		"status": record.status().as_str(),
		"updated_at": record.updated_at(),
	})
}

pub(super) fn architecture_recovery_program_payload(record: &ExecutionProgramRecord) -> Value {
	execution_architecture_recovery::json!({
		"program_id": record.program_id(),
		"source_contract_id": record.source_contract_id(),
		"updated_at": record.updated_at(),
	})
}

pub(super) fn architecture_recovery_validation_failures(
	stop: &LoopGuardrailStopRequested,
	error: &Report,
) -> Value {
	execution_architecture_recovery::json!({
		"guardrail_reason": stop.reason.error_class(),
		"source_error_class": stop.source_error_class.as_deref(),
		"error_summary": truncate_private_diagnostic_text(&error.to_string()),
	})
}
