mod checkpoint;
mod failure;

use serde_json::Value;

use crate::{
	orchestrator::{
		self, PhaseGoalKind, RepoGateCommandOutcome, ResolvedRepoGate, Result,
		execution_phase_goal::{
			acceptance::{
				self, ValidationDecision, ValidationEvidence, validation_evidence_blocker_count,
				validation_evidence_has_non_goal_violation,
			},
			controller::RepoGatePhaseGoalController,
		},
	},
	state::PrivateExecutionEvent,
};

impl RepoGatePhaseGoalController<'_> {
	pub(in crate::orchestrator::execution_phase_goal) fn evaluate_validation_evidence(
		&self,
		phase: PhaseGoalKind,
		repo_gate: &ResolvedRepoGate<'_>,
		repo_gate_outcome: &RepoGateCommandOutcome,
	) -> Result<ValidationEvidence> {
		let fingerprint =
			orchestrator::loop_guardrail_worktree_fingerprint(&self.issue_run.worktree.path)?;
		let head_sha = fingerprint.as_ref().map(|value| value.head_sha.clone());
		let changed_surfaces =
			acceptance::validation_evidence_changed_surfaces(&self.issue_run.worktree.path);
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
		let checkpoint_present = checkpoint.is_some();
		let blocker_count = checkpoint_payload.map_or(0, validation_evidence_blocker_count);
		let non_goal_violation =
			checkpoint_payload.is_some_and(validation_evidence_has_non_goal_violation);
		let objective_covered = (!checkpoint_present || checkpoint_matches_head)
			&& effective_delta_present
			&& blocker_count == 0;
		let non_goal_passed = !non_goal_violation;
		let validation_passed = true;
		let reason_code = acceptance::validation_evidence_reason_code(
			checkpoint_present,
			checkpoint_matches_head,
			effective_delta_present,
			non_goal_passed,
			blocker_count,
		);
		let decision = if reason_code == "accepted" {
			ValidationDecision::Pass
		} else {
			ValidationDecision::Fail
		};

		Ok(ValidationEvidence {
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
