use serde_json::Value;

use crate::orchestrator::execution_phase_goal::{
	acceptance::PhaseAcceptanceCheck, controller::RepoGatePhaseGoalController,
};
use crate::orchestrator::{
	self, LaneDecisionSnapshot, LaneNextAction, PHASE_ACCEPTANCE_CHECK_EVENT_TYPE, PhaseGoalKind,
	PhaseGoalSpec, RepoGateTrackedRewriteDecision, Result,
};

impl RepoGatePhaseGoalController<'_> {
	pub(super) fn persist_next_phase_goal(&self, goal: &PhaseGoalSpec, reason: &str) -> Result<()> {
		self.state_store.append_private_execution_event(
			self.project.service_id(),
			&self.issue_run.issue.id,
			&self.issue_run.run_id,
			self.issue_run.attempt_number,
			"phase_goal_next",
			orchestrator::json!({
				"schema": "decodex.phase_goal_signal/1",
				"phase": goal.phase.as_str(),
				"reason": reason,
			}),
		)?;

		Ok(())
	}

	pub(super) fn record_phase_goal_transition(
		&self,
		phase: PhaseGoalKind,
		signal: &str,
		payload: Value,
	) -> Result<()> {
		self.state_store.append_private_execution_event(
			self.project.service_id(),
			&self.issue_run.issue.id,
			&self.issue_run.run_id,
			self.issue_run.attempt_number,
			"phase_goal_transition",
			orchestrator::json!({
				"schema": "decodex.phase_goal_signal/1",
				"phase": phase.as_str(),
				"signal": signal,
				"payload": payload,
			}),
		)?;

		Ok(())
	}

	pub(super) fn record_lane_decision_snapshot(
		&self,
		snapshot: &LaneDecisionSnapshot,
		action: LaneNextAction,
		reason: &str,
	) -> Result<()> {
		self.state_store.append_private_execution_event(
			self.project.service_id(),
			&self.issue_run.issue.id,
			&self.issue_run.run_id,
			self.issue_run.attempt_number,
			"lane_decision",
			snapshot.to_json(action, reason),
		)?;

		Ok(())
	}

	pub(super) fn record_phase_acceptance_check(&self, check: &PhaseAcceptanceCheck) -> Result<()> {
		self.state_store.append_private_execution_event(
			self.project.service_id(),
			&self.issue_run.issue.id,
			&self.issue_run.run_id,
			self.issue_run.attempt_number,
			PHASE_ACCEPTANCE_CHECK_EVENT_TYPE,
			orchestrator::json!({
				"schema": "decodex.phase_acceptance_check/1",
				"phase": check.phase.as_str(),
				"decision": check.decision.as_str(),
				"reason_code": check.reason_code,
				"objective_coverage": {
					"covered": check.objective_covered,
					"checkpoint_record_id": check.checkpoint_record_id,
					"checkpoint_head_sha": check.checkpoint_head_sha.as_deref(),
					"worktree_head_sha": check.worktree_head_sha.as_deref(),
				},
				"effective_delta": {
					"present": check.effective_delta_present,
					"changed_surfaces": &check.changed_surfaces,
				},
				"non_goal_check": {
					"passed": check.non_goal_passed,
					"blocker_count": check.blocker_count,
				},
				"validation_evidence": {
					"repo_gate_passed": check.validation_passed,
					"repo_gate_profile": check.repo_gate_profile.as_deref(),
					"canonicalize_commands": &check.canonicalize_commands,
					"verify_commands": &check.verify_commands,
					"tracked_rewrites": check
						.repo_gate_tracked_rewrites
						.as_ref()
						.map(RepoGateTrackedRewriteDecision::to_json),
				},
				"next_action": check.next_action(),
			}),
		)?;

		Ok(())
	}
}
