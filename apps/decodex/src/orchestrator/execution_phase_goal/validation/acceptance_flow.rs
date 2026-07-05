mod checkpoint;
mod failure;

use serde_json::Value;

use crate::{
	orchestrator::{
		self, PhaseGoalKind, RepoGateCommandOutcome, ResolvedRepoGate, Result,
		execution_phase_goal::{
			acceptance::{
				self, PhaseAcceptanceCheck, PhaseAcceptanceDecision,
				phase_acceptance_blocker_count, phase_acceptance_docs_impact_valid,
				phase_acceptance_has_non_goal_violation,
			},
			controller::RepoGatePhaseGoalController,
		},
	},
	state::PrivateExecutionEvent,
};

impl RepoGatePhaseGoalController<'_> {
	pub(in crate::orchestrator::execution_phase_goal) fn evaluate_phase_acceptance(
		&self,
		phase: PhaseGoalKind,
		repo_gate: &ResolvedRepoGate<'_>,
		repo_gate_outcome: &RepoGateCommandOutcome,
	) -> Result<PhaseAcceptanceCheck> {
		let fingerprint =
			orchestrator::loop_guardrail_worktree_fingerprint(&self.issue_run.worktree.path)?;
		let head_sha = fingerprint.as_ref().map(|value| value.head_sha.clone());
		let changed_surfaces =
			acceptance::phase_acceptance_changed_surfaces(&self.issue_run.worktree.path);
		let effective_delta_present =
			fingerprint.as_ref().is_some_and(|value| value.effective_delta_present)
				|| !changed_surfaces.is_empty();
		let checkpoint = self.latest_progress_checkpoint()?;
		let checkpoint_payload = checkpoint.as_ref().map(PrivateExecutionEvent::payload);
		let checkpoint_head_sha = checkpoint_payload
			.and_then(|payload| payload.get("head_sha"))
			.and_then(Value::as_str)
			.map(str::to_owned);
		let checkpoint_matches_head = head_sha
			.as_deref()
			.zip(checkpoint_head_sha.as_deref())
			.is_some_and(|(head, checkpoint_head)| head == checkpoint_head);
		let docs_impact_valid = checkpoint_payload
			.and_then(|payload| payload.get("docs_impact"))
			.and_then(Value::as_str)
			.is_some_and(phase_acceptance_docs_impact_valid);
		let blocker_count = checkpoint_payload.map_or(0, phase_acceptance_blocker_count);
		let non_goal_violation =
			checkpoint_payload.is_some_and(phase_acceptance_has_non_goal_violation);
		let objective_covered = checkpoint.is_some()
			&& checkpoint_matches_head
			&& docs_impact_valid
			&& blocker_count == 0;
		let non_goal_passed = !non_goal_violation;
		let validation_passed = true;
		let reason_code = acceptance::phase_acceptance_reason_code(
			checkpoint.is_some(),
			checkpoint_matches_head,
			docs_impact_valid,
			effective_delta_present,
			non_goal_passed,
			blocker_count,
		);
		let decision = if reason_code == "accepted" {
			PhaseAcceptanceDecision::Pass
		} else {
			PhaseAcceptanceDecision::Fail
		};

		Ok(PhaseAcceptanceCheck {
			phase,
			decision,
			reason_code,
			objective_covered,
			effective_delta_present,
			changed_surfaces,
			non_goal_passed,
			validation_passed,
			repo_gate_profile: repo_gate.profile_name().map(str::to_owned),
			canonicalize_commands: repo_gate.canonicalize_commands().to_vec(),
			verify_commands: repo_gate.verify_commands().to_vec(),
			repo_gate_tracked_rewrites: repo_gate_outcome.tracked_rewrite_decision().cloned(),
			checkpoint_record_id: checkpoint.as_ref().map(PrivateExecutionEvent::record_id),
			checkpoint_head_sha,
			worktree_head_sha: head_sha,
			blocker_count,
		})
	}
}
