use serde_json::Value;

use crate::{
	agent::ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME,
	config::ServiceConfig,
	orchestrator::{
		ARCHITECTURE_RECOVERY_PACKET_EVENT_TYPE, ARCHITECTURE_RECOVERY_STARTED_EVENT_TYPE,
		ARCHITECTURE_RECOVERY_TERMINAL_EVENT_TYPE, IssueDispatchMode, IssueRunPlan,
	},
	state::StateStore,
};

pub(super) fn build_retry_recovery_context(dispatch_mode: IssueDispatchMode) -> Option<String> {
	(dispatch_mode == IssueDispatchMode::Retry).then(|| {
		String::from(
			"Recovery context\n- This is retry-style re-entry after a prior attempt stopped or could not prove live execution.\n- Treat the current worktree, tracker state, protocol events, and marker files as the durable source of truth. Do not assume in-memory model output or tool results survived.\n- Before editing, inspect the current branch, diff, and recent validation evidence, reconcile partial work already present, and continue from that state instead of restarting from scratch.",
		)
	})
}

pub(super) fn build_architecture_recovery_context(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
) -> Option<String> {
	let events = match state_store
		.list_private_execution_events_for_issue(project.service_id(), &issue_run.issue.id)
	{
		Ok(events) => events,
		Err(error) => {
			tracing::warn!(
				?error,
				issue = issue_run.issue.identifier,
				run_id = issue_run.run_id,
				"Prompt could not read architecture recovery evidence."
			);

			return None;
		},
	};
	let event = events.iter().rev().find(|event| {
		matches!(
			event.event_type(),
			ARCHITECTURE_RECOVERY_PACKET_EVENT_TYPE
				| ARCHITECTURE_RECOVERY_STARTED_EVENT_TYPE
				| ARCHITECTURE_RECOVERY_TERMINAL_EVENT_TYPE
		)
	})?;

	if event.event_type() != ARCHITECTURE_RECOVERY_STARTED_EVENT_TYPE {
		return None;
	}

	let payload = event.payload();
	let guardrail_reason =
		payload.get("guardrail_reason").and_then(Value::as_str).unwrap_or("loop_guardrail");
	let recovery_attempt = payload
		.get("recovery_budget")
		.and_then(|budget| budget.get("attempt"))
		.and_then(Value::as_u64)
		.unwrap_or(1);
	let recovery_max = payload
		.get("recovery_budget")
		.and_then(|budget| budget.get("max_attempts"))
		.and_then(Value::as_u64)
		.unwrap_or(1);
	let policy_decision =
		payload.get("boundary_policy_decision").and_then(Value::as_str).unwrap_or("auto_continue");
	let requires_enhanced_evidence = payload
		.get("requires_enhanced_evidence")
		.and_then(Value::as_bool)
		.unwrap_or(matches!(policy_decision, "requires_enhanced_evidence" | "block_landing"));
	let blocks_landing = payload
		.get("blocks_landing")
		.and_then(Value::as_bool)
		.unwrap_or(policy_decision == "block_landing");
	let mut policy_guidance = format!("Authority policy `{policy_decision}` applies");

	if requires_enhanced_evidence {
		policy_guidance.push_str("; preserve enhanced evidence before review handoff or landing");
	}
	if blocks_landing {
		policy_guidance.push_str(
			"; keep landing blocked until validation or review-policy evidence is restored",
		);
	}

	policy_guidance.push('.');

	Some(format!(
		"Architecture recovery context\n- Decodex recorded `architecture_recovery_started` for guardrail `{guardrail_reason}` after an Authority Boundary Check returned policy `{policy_decision}`.\n- This is autonomous architecture recovery attempt {recovery_attempt} of {recovery_max}; start a materially different implementation strategy instead of repeating the ineffective repair.\n- {policy_guidance}\n- Preserve the accepted Decision Contract, public API/config behavior, and validation/review gates. Do not ask the user through chat while detached; use manual attention only if the next viable action crosses authority."
	))
}

pub(super) fn build_external_repair_architecture_guidance(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
) -> String {
	let lifecycle_record = match state_store.review_lifecycle_record(
		project.service_id(),
		&issue_run.issue.id,
		&issue_run.worktree.branch_name,
	) {
		Ok(Some(lifecycle_record)) => lifecycle_record,
		Ok(None) => return String::new(),
		Err(error) => {
			tracing::warn!(
				?error,
				issue = issue_run.issue.identifier,
				run_id = issue_run.run_id,
				worktree_path = %issue_run.worktree.path.display(),
				"Retained review prompt could not read the runtime handoff; omitting architecture guidance."
			);

			return String::new();
		},
	};

	if lifecycle_record.external_round_count() < 4 {
		return String::new();
	}

	format!(
		"- This retained repair is GitHub Review round {}. Before another patch-only cycle, decide whether the repeated churn points to an architectural or root-cause defect that local patching will not converge.\n- If it is architectural, take the manual-attention path instead of continuing patch-on-patch repair.\n- If it is not architectural and the findings are still normal retained review work, continue this repair normally; a successful `{}` will reset the GitHub Review round budget.\n",
		lifecycle_record.external_round_count(),
		ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME
	)
}
