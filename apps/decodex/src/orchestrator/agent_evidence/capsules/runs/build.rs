use std::{collections::BTreeSet, path::Path};

use crate::orchestrator::{
	OperatorRunStatus,
	agent_evidence::{
		self, AGENT_RUN_CAPSULE_SCHEMA, AgentEvidenceProjectView, AgentRunCapsule,
		AgentRunLedgerOutcome,
		capsules::runs::{diagnosis, ledger},
	},
};

pub(crate) fn build_run_capsules(
	project_view: &AgentEvidenceProjectView<'_>,
	generated_at: &str,
	runs_dir: &Path,
	month_bucket: &str,
) -> Vec<AgentRunCapsule> {
	let mut run_ids = BTreeSet::new();
	let mut capsules = Vec::new();

	for run in project_view.current_lanes.iter().chain(project_view.recent_runs.iter()).copied() {
		if run_ids.insert(run.run_id.clone()) {
			capsules.push(agent_run_capsule(
				project_view.project_id,
				generated_at,
				runs_dir,
				month_bucket,
				run,
				ledger::ledger_outcome_for_run(run, project_view),
			));
		}
	}
	for lane in &project_view.history_lanes {
		for run in &lane.attempts {
			if run_ids.insert(run.run_id.clone()) {
				capsules.push(agent_run_capsule(
					project_view.project_id,
					generated_at,
					runs_dir,
					month_bucket,
					run,
					Some(ledger::agent_run_ledger_outcome(&lane.ledger_outcome)),
				));
			}
		}
	}

	capsules
}

fn agent_run_capsule(
	project_id: &str,
	generated_at: &str,
	runs_dir: &Path,
	month_bucket: &str,
	run: &OperatorRunStatus,
	ledger_outcome: Option<AgentRunLedgerOutcome>,
) -> AgentRunCapsule {
	let path = agent_evidence::run_capsule_path(runs_dir, month_bucket, &run.run_id);
	let diagnosis = diagnosis::agent_run_diagnosis(run);
	let private_evidence = agent_evidence::agent_private_evidence_ref(run);

	AgentRunCapsule {
		schema: AGENT_RUN_CAPSULE_SCHEMA,
		evidence_ref: agent_evidence::run_evidence_ref(project_id, &run.run_id),
		project_id: project_id.to_owned(),
		generated_at: generated_at.to_owned(),
		path: path.display().to_string(),
		run_id: run.run_id.clone(),
		issue_id: run.issue_id.clone(),
		issue_identifier: run.issue_identifier.clone(),
		title: run.title.clone(),
		attempt_number: run.attempt_number,
		status: run.status.clone(),
		attempt_status: run.attempt_status.clone(),
		phase: run.phase.clone(),
		wait_reason: run.wait_reason.clone(),
		current_operation: run.current_operation.clone(),
		queue_lease_state: run.queue_lease_state.clone(),
		execution_liveness: run.execution_liveness.clone(),
		ownership_state: run.ownership_state.clone(),
		liveness_state: run.liveness_state.clone(),
		policy_state: run.policy_state.clone(),
		terminalization_state: run.terminalization_state.clone(),
		lane_control_next_action: run.lane_control_next_action.clone(),
		lane_control_conditions: run.lane_control_conditions.clone(),
		run_lease: run.run_lease,
		continuation_pending: run.continuation_pending,
		suspected_stall: run.suspected_stall,
		thread_id: run.thread_id.clone(),
		turn_id: run.turn_id.clone(),
		thread_status: run.thread_status.clone(),
		thread_active_flags: run.thread_active_flags.clone(),
		interactive_requested: run.interactive_requested,
		process_id: run.process_id,
		process_alive: run.process_alive,
		process_liveness_reason: run.process_liveness_reason.clone(),
		event_count: run.event_count,
		last_event_type: run.last_event_type.clone(),
		last_event_at: run.last_event_at.clone(),
		last_run_activity_at: run.last_run_activity_at.clone(),
		last_protocol_activity_at: run.last_protocol_activity_at.clone(),
		last_progress_at: run.last_progress_at.clone(),
		idle_for_seconds: run.idle_for_seconds,
		protocol_idle_for_seconds: run.protocol_idle_for_seconds,
		retry_kind: run.retry_kind.clone(),
		next_retry_at: run.next_retry_at.clone(),
		effective_model: run.effective_model.clone(),
		effective_model_provider: run.effective_model_provider.clone(),
		effective_cwd: run.effective_cwd.clone(),
		effective_approval_policy: run.effective_approval_policy.clone(),
		effective_approvals_reviewer: run.effective_approvals_reviewer.clone(),
		effective_sandbox_mode: run.effective_sandbox_mode.clone(),
		branch_name: run.branch_name.clone(),
		worktree_path: run.worktree_path.clone(),
		private_evidence,
		ledger_outcome,
		diagnosis,
	}
}
