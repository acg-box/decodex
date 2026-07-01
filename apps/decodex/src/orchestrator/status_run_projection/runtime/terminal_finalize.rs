#[allow(clippy::wildcard_imports)] use super::*;

pub(in crate::orchestrator) fn operator_run_terminal_finalize_projection(
	loop_evidence: &ProjectLoopEvidenceSnapshot,
	run: &ProjectRunStatus,
) -> Option<OperatorTerminalFinalizeProjection> {
	let events = loop_evidence.private_events(run.issue_id(), run.run_id(), run.attempt_number());
	let path = events
		.iter()
		.rev()
		.find(|event| event.event_type() == "terminal_finalize")
		.and_then(|event| event.payload().get("path"))
		.and_then(Value::as_str)?;

	match path {
		"review_handoff" => Some(OperatorTerminalFinalizeProjection {
			status: "review_handoff_pending",
			phase: "terminal_pending",
			wait_reason: review_handoff_terminal_finalize_wait_reason(loop_evidence, run, events),
			current_operation: RUN_OPERATION_REVIEW_WRITEBACK,
		}),
		"review_repair" => Some(OperatorTerminalFinalizeProjection {
			status: "review_repair_pending",
			phase: "terminal_pending",
			wait_reason: review_repair_terminal_finalize_wait_reason(loop_evidence, run, events),
			current_operation: RUN_OPERATION_REVIEW_WRITEBACK,
		}),
		"closeout" => Some(OperatorTerminalFinalizeProjection {
			status: "closeout_pending",
			phase: "terminal_pending",
			wait_reason: "closeout_writeback",
			current_operation: RUN_OPERATION_REVIEW_WRITEBACK,
		}),
		"manual_attention" => Some(OperatorTerminalFinalizeProjection {
			status: "manual_attention_pending",
			phase: "terminal_pending",
			wait_reason: "manual_attention_writeback",
			current_operation: RUN_OPERATION_REVIEW_WRITEBACK,
		}),
		_ => None,
	}
}

pub(in crate::orchestrator) fn review_handoff_terminal_finalize_wait_reason(
	loop_evidence: &ProjectLoopEvidenceSnapshot,
	run: &ProjectRunStatus,
	events: &[PrivateExecutionEvent],
) -> &'static str {
	let Some(intent) = events.iter().rev().find(|event| {
		let payload = event.payload();

		event.event_type() == "review_completion_intent"
			&& payload.get("path").and_then(Value::as_str) == Some("review_handoff")
			&& payload.get("mode").and_then(Value::as_str) == Some("handoff")
			&& payload.get("pr_url").and_then(Value::as_str).is_some()
			&& payload.get("pr_head_oid").and_then(Value::as_str).is_some()
			&& payload.get("worktree_path").and_then(Value::as_str).is_some()
	}) else {
		return "review_handoff_writeback";
	};
	let Some(branch) = intent.payload().get("branch").and_then(Value::as_str) else {
		return "review_handoff_writeback";
	};

	if loop_evidence.review_lifecycle_record(run.issue_id(), branch).is_none() {
		return "review_handoff_writeback_missing_lifecycle_marker";
	}

	"review_handoff_writeback"
}

pub(in crate::orchestrator) fn review_repair_terminal_finalize_wait_reason(
	loop_evidence: &ProjectLoopEvidenceSnapshot,
	run: &ProjectRunStatus,
	events: &[PrivateExecutionEvent],
) -> &'static str {
	let Some(intent) = events.iter().rev().find(|event| {
		let payload = event.payload();

		event.event_type() == "review_completion_intent"
			&& payload.get("path").and_then(Value::as_str) == Some("review_repair")
			&& payload.get("mode").and_then(Value::as_str) == Some("repair")
			&& payload.get("pr_url").and_then(Value::as_str).is_some()
			&& payload.get("pr_head_ref").and_then(Value::as_str).is_some()
			&& payload.get("pr_head_oid").and_then(Value::as_str).is_some()
			&& payload.get("worktree_path").and_then(Value::as_str).is_some()
	}) else {
		return "review_repair_writeback";
	};
	let payload = intent.payload();
	let Some(branch) = payload.get("branch").and_then(Value::as_str) else {
		return "review_repair_writeback";
	};
	let Some(pr_url) = payload.get("pr_url").and_then(Value::as_str) else {
		return "review_repair_writeback";
	};
	let Some(pr_head_ref) = payload.get("pr_head_ref").and_then(Value::as_str) else {
		return "review_repair_writeback";
	};
	let Some(pr_head_oid) = payload.get("pr_head_oid").and_then(Value::as_str) else {
		return "review_repair_writeback";
	};
	let Some(lifecycle_record) = loop_evidence.review_lifecycle_record(run.issue_id(), branch)
	else {
		return "review_repair_writeback_missing_lifecycle_marker";
	};

	if lifecycle_record.pr_url() != pr_url
		|| lifecycle_record.pr_head_ref_name() != pr_head_ref
		|| lifecycle_record.pr_head_oid() != pr_head_oid
		|| lifecycle_record.head_sha() != pr_head_oid
	{
		return "review_repair_writeback_stale_lifecycle_marker";
	}

	"review_repair_writeback"
}
