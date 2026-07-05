use serde_json::Value;

use crate::orchestrator::{
	PhaseGoalKind, RepoGateFailureDisposition, RetryKind,
	lane_decision::{LaneDecisionSnapshot, LaneNextAction, json_projection, projection},
};

impl LaneDecisionSnapshot {
	pub(in crate::orchestrator) fn to_json(&self, action: LaneNextAction, reason: &str) -> Value {
		let decision = projection::decide_lane_next_action(self);

		serde_json::json!({
			"schema": "decodex.lane_decision_snapshot/1",
			"issue_identifier": self.issue_identifier,
			"run_id": self.run_id,
			"attempt_number": self.attempt_number,
			"dispatch_mode": self.dispatch_mode.as_str(),
			"active_phase": self.active_phase.map(PhaseGoalKind::as_str),
			"continuation_pending": self.continuation_pending,
			"retry_kind": self.retry_kind.map(RetryKind::as_str),
			"retry_budget_consumed": self.retry_budget_consumed,
			"progress_blocker_count": self.progress_blocker_count,
			"non_goal_violation": self.non_goal_violation,
			"phase_acceptance_failure": self.phase_acceptance_failure,
			"repo_gate_disposition": self.repo_gate_disposition.map(RepoGateFailureDisposition::as_str),
			"scope_envelope_violation": self.scope_envelope_violation,
			"ambiguous_lineage": self.ambiguous_lineage,
			"terminal_evidence_present": self.terminal_evidence_present,
			"next_action": action.as_str(),
			"reason": reason,
			"kernel_decision": json_projection::owned_lane_decision_to_json(&decision.kernel_decision),
		})
	}
}
