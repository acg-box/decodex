use serde_json::json;

use crate::orchestrator::execution_architecture_recovery::{
	self, ARCHITECTURE_RECOVERY_BUDGET, ARCHITECTURE_RECOVERY_PACKET_EVENT_TYPE,
	ARCHITECTURE_RECOVERY_PACKET_SCHEMA, ARCHITECTURE_RECOVERY_STARTED_EVENT_TYPE,
	ARCHITECTURE_RECOVERY_TERMINAL_EVENT_TYPE, ArchitectureRecoveryPacketInput,
	ArchitectureRecoveryTerminalEventInput, AuthorityBoundaryPolicyDecision,
	AuthorityDecisionOption, AuthorityDecisionRequestInput, DecisionContractRecord,
	ExecutionProgramRecord, IssueRunPlan, LOOP_GUARDRAIL_CONVERGENCE_BUDGET,
	LoopGuardrailStopRequested, Path, Report, Result, ServiceConfig, StateStore, Value,
	loop_guardrail_effective_status, truncate_private_diagnostic_text,
};

pub(super) fn record_architecture_recovery_packet(
	state_store: &StateStore,
	input: ArchitectureRecoveryPacketInput<'_>,
) -> Result<()> {
	let programs = architecture_recovery_programs_for_contracts(
		state_store,
		input.project.service_id(),
		input.contracts,
	)?;
	let retained = architecture_recovery_retained_worktree(&input.issue_run.worktree.path)?;
	let review =
		architecture_recovery_review_findings(state_store, input.project, input.issue_run)?;

	state_store
		.append_private_execution_event(
			input.project.service_id(),
			&input.issue_run.issue.id,
			&input.issue_run.run_id,
			input.issue_run.attempt_number,
			ARCHITECTURE_RECOVERY_PACKET_EVENT_TYPE,
			execution_architecture_recovery::json!({
				"schema": ARCHITECTURE_RECOVERY_PACKET_SCHEMA,
				"record_version": 1,
				"state": input.reason_code,
				"reason_code": input.reason_code,
				"issue": architecture_recovery_issue_payload(input.issue_run),
				"run": architecture_recovery_run_payload(input.issue_run),
				"decision_contract_context": input.contracts
					.iter()
					.map(architecture_recovery_contract_payload)
					.collect::<Vec<_>>(),
				"execution_program_context": programs
					.iter()
					.map(architecture_recovery_program_payload)
					.collect::<Vec<_>>(),
				"retained_worktree": retained,
				"validation_failures": architecture_recovery_validation_failures(
					input.loop_guardrail_stop,
					input.error,
				),
				"review_findings": review,
				"prior_recovery_attempts": {
					"started_count": input.prior_started_count,
				},
				"recovery_budget": {
					"attempt": input.recovery_attempt_number,
					"max_attempts": ARCHITECTURE_RECOVERY_BUDGET,
				},
				"loop_guardrail": {
					"reason": input.loop_guardrail_stop.reason.error_class(),
					"consecutive_count": input.loop_guardrail_stop.consecutive_count,
					"threshold": LOOP_GUARDRAIL_CONVERGENCE_BUDGET,
					"fingerprint": input.loop_guardrail_stop.fingerprint.as_str(),
					"source_error_class": input.loop_guardrail_stop.source_error_class.as_deref(),
				},
				"authority_boundary_check": {
					"record_id": input.boundary_check_record_id,
					"disposition": input.boundary_disposition.as_str(),
					"policy_decision": input.boundary_policy_decision.as_str(),
					"requires_enhanced_evidence": input
						.boundary_policy_decision
						.requires_enhanced_evidence(),
					"blocks_landing": input.boundary_policy_decision.blocks_landing(),
					"reason": input.boundary_final_reason,
				},
			}),
		)
		.map(|_| ())
}

pub(super) fn record_architecture_recovery_started_event(
	state_store: &StateStore,
	project: &ServiceConfig,
	issue_run: &IssueRunPlan,
	stop: &LoopGuardrailStopRequested,
	boundary_check_record_id: i64,
	boundary_policy_decision: AuthorityBoundaryPolicyDecision,
	recovery_attempt_number: usize,
) -> Result<()> {
	state_store
		.append_private_execution_event(
			project.service_id(),
			&issue_run.issue.id,
			&issue_run.run_id,
			issue_run.attempt_number,
			ARCHITECTURE_RECOVERY_STARTED_EVENT_TYPE,
			execution_architecture_recovery::json!({
				"schema": "decodex.architecture_recovery_started/1",
				"record_version": 1,
				"reason_code": "architecture_recovery_started",
				"guardrail_reason": stop.reason.error_class(),
				"authority_boundary_check_record_id": boundary_check_record_id,
				"boundary_policy_decision": boundary_policy_decision.as_str(),
				"requires_enhanced_evidence": boundary_policy_decision.requires_enhanced_evidence(),
				"blocks_landing": boundary_policy_decision.blocks_landing(),
				"recovery_budget": {
					"attempt": recovery_attempt_number,
					"max_attempts": ARCHITECTURE_RECOVERY_BUDGET,
				},
				"next_strategy": "materially_different_architecture_recovery",
			}),
		)
		.map(|_| ())
}

pub(super) fn record_architecture_recovery_terminal_outcome(
	state_store: &StateStore,
	input: ArchitectureRecoveryTerminalEventInput<'_>,
) -> Result<()> {
	record_architecture_recovery_terminal_event(state_store, &input)?;

	if input.boundary_policy_decision.allows_autonomous_recovery() {
		return Ok(());
	}

	let decision_request_id = format!(
		"{}-{}-{}-{}",
		input.issue_run.issue.identifier,
		input.issue_run.run_id,
		input.issue_run.attempt_number,
		input.reason_code
	);

	execution_architecture_recovery::record_authority_decision_request_private_event(
		state_store,
		architecture_recovery_decision_request_input(
			input.project,
			input.issue_run,
			input.stop,
			input.boundary_check_record_id,
			&decision_request_id,
			input.reason_code,
			input.boundary_final_reason,
		),
	)
	.map(|_| ())
}

pub(super) fn architecture_recovery_goal_detail(
	stop: &LoopGuardrailStopRequested,
	recovery_attempt_number: usize,
	policy_decision: AuthorityBoundaryPolicyDecision,
) -> String {
	format!(
		"Loop guardrail `{}` stopped the current ineffective strategy after {} matching observations. Decodex recorded an Architecture Recovery Packet and an Authority Boundary Check with policy `{}`; use autonomous architecture recovery attempt {} of {}. Start a materially different implementation strategy, preserve the accepted Decision Contract and all validation/review gates, and {}.",
		stop.reason.error_class(),
		stop.consecutive_count,
		policy_decision.as_str(),
		recovery_attempt_number,
		ARCHITECTURE_RECOVERY_BUDGET,
		architecture_recovery_policy_recovery_guidance(policy_decision)
	)
}

pub(crate) fn architecture_recovery_retry_next_action(
	policy_decision: AuthorityBoundaryPolicyDecision,
) -> &'static str {
	match policy_decision {
		AuthorityBoundaryPolicyDecision::AutoContinue =>
			"decodex recorded authority policy `auto_continue` and will retry with a materially different architecture recovery strategy",
		AuthorityBoundaryPolicyDecision::RequiresEnhancedEvidence =>
			"decodex recorded authority policy `requires_enhanced_evidence` and will retry with a materially different architecture recovery strategy while preserving enhanced evidence before review handoff or landing",
		AuthorityBoundaryPolicyDecision::BlockLanding =>
			"decodex recorded authority policy `block_landing` and will retry with a materially different architecture recovery strategy while landing remains blocked until validation or review-policy evidence is restored",
		AuthorityBoundaryPolicyDecision::RequiresHumanDecision =>
			"decodex recorded authority policy `requires_human_decision` and requires human attention before retrying",
	}
}

fn architecture_recovery_programs_for_contracts(
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

fn architecture_recovery_retained_worktree(worktree_path: &Path) -> Result<Value> {
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

fn architecture_recovery_review_findings(
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

fn architecture_recovery_issue_payload(issue_run: &IssueRunPlan) -> Value {
	execution_architecture_recovery::json!({
		"id": issue_run.issue.id.as_str(),
		"identifier": issue_run.issue.identifier.as_str(),
		"title": issue_run.issue.title.as_str(),
	})
}

fn architecture_recovery_run_payload(issue_run: &IssueRunPlan) -> Value {
	execution_architecture_recovery::json!({
		"run_id": issue_run.run_id.as_str(),
		"attempt_number": issue_run.attempt_number,
		"branch": issue_run.worktree.branch_name.as_str(),
		"dispatch_mode": issue_run.dispatch_mode.as_str(),
	})
}

fn architecture_recovery_contract_payload(record: &DecisionContractRecord) -> Value {
	execution_architecture_recovery::json!({
		"contract_id": record.contract_id(),
		"source_issue_id": record.source_issue_id(),
		"status": record.status().as_str(),
		"updated_at": record.updated_at(),
	})
}

fn architecture_recovery_program_payload(record: &ExecutionProgramRecord) -> Value {
	execution_architecture_recovery::json!({
		"program_id": record.program_id(),
		"source_contract_id": record.source_contract_id(),
		"updated_at": record.updated_at(),
	})
}

fn architecture_recovery_validation_failures(
	stop: &LoopGuardrailStopRequested,
	error: &Report,
) -> Value {
	execution_architecture_recovery::json!({
		"guardrail_reason": stop.reason.error_class(),
		"source_error_class": stop.source_error_class.as_deref(),
		"error_summary": truncate_private_diagnostic_text(&error.to_string()),
	})
}

fn record_architecture_recovery_terminal_event(
	state_store: &StateStore,
	input: &ArchitectureRecoveryTerminalEventInput<'_>,
) -> Result<()> {
	state_store
		.append_private_execution_event(
			input.project.service_id(),
			&input.issue_run.issue.id,
			&input.issue_run.run_id,
			input.issue_run.attempt_number,
			ARCHITECTURE_RECOVERY_TERMINAL_EVENT_TYPE,
			execution_architecture_recovery::json!({
				"schema": "decodex.architecture_recovery_terminal/1",
				"record_version": 1,
				"reason_code": input.reason_code,
				"guardrail_reason": input.stop.reason.error_class(),
				"authority_boundary_check_record_id": input.boundary_check_record_id,
				"boundary_disposition": input.boundary_disposition.as_str(),
				"boundary_policy_decision": input.boundary_policy_decision.as_str(),
				"requires_enhanced_evidence": input
					.boundary_policy_decision
					.requires_enhanced_evidence(),
				"blocks_landing": input.boundary_policy_decision.blocks_landing(),
				"recovery_budget": {
					"attempt": input.recovery_attempt_number,
					"max_attempts": ARCHITECTURE_RECOVERY_BUDGET,
				},
			}),
		)
		.map(|_| ())
}

fn architecture_recovery_decision_request_input<'a>(
	project: &'a ServiceConfig,
	issue_run: &'a IssueRunPlan,
	stop: &'a LoopGuardrailStopRequested,
	boundary_check_record_id: i64,
	decision_request_id: &'a str,
	reason_code: &'a str,
	final_reason: &'a str,
) -> AuthorityDecisionRequestInput<'a> {
	AuthorityDecisionRequestInput {
		project_id: project.service_id(),
		issue_id: &issue_run.issue.id,
		issue_identifier: &issue_run.issue.identifier,
		run_id: &issue_run.run_id,
		attempt_number: issue_run.attempt_number,
		boundary_check_record_id,
		decision_request_id,
		reason_code,
		boundary_type: "architecture_recovery",
		proposed_change: "Continue loop recovery with a materially different architecture strategy.",
		why_exceeds_authority: final_reason,
		options: vec![
			AuthorityDecisionOption {
				label: "Authorize recovery",
				description: "Update the issue, Decision Contract, or policy to allow this recovery.",
			},
			AuthorityDecisionOption {
				label: "Keep stopped",
				description: "Leave the lane in manual attention until the boundary is resolved.",
			},
		],
		recommendation: "Resolve the authority boundary before requeueing the lane.",
		resume_condition: "Accept, reject, or revise the requested authority in the issue, Decision Contract, or project policy before clearing needs-attention.",
		retained_worktree_evidence: vec![issue_run.worktree.branch_name.as_str()],
		retained_diff_evidence: vec![stop.fingerprint.as_str()],
		recovery_attempt_context: vec![stop.reason.error_class()],
	}
}

fn architecture_recovery_policy_recovery_guidance(
	policy_decision: AuthorityBoundaryPolicyDecision,
) -> &'static str {
	match policy_decision {
		AuthorityBoundaryPolicyDecision::AutoContinue =>
			"request human attention only if the next viable action would change product behavior, public API/config contract, security, data, credential, billing, validation standards, or accepted authority",
		AuthorityBoundaryPolicyDecision::RequiresEnhancedEvidence =>
			"preserve enhanced evidence for the changed high-risk surfaces before review handoff or landing",
		AuthorityBoundaryPolicyDecision::BlockLanding =>
			"keep landing blocked until validation or review-policy evidence is restored",
		AuthorityBoundaryPolicyDecision::RequiresHumanDecision =>
			"request human attention before continuing recovery",
	}
}
