mod checkpoint;
mod failure;

use serde_json::Value;

use crate::{
	lane_authority::{LaneId, NoEffectiveDeltaFacts, build_canonical_patch_set},
	orchestrator::{
		self, PhaseGoalKind, RepoGateCommandOutcome, ResolvedRepoGate, Result,
		execution_failure::LoopGuardrailWorktreeFingerprint,
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
		let no_effective_delta = (reason_code == "no_effective_delta")
			.then(|| self.no_effective_delta_authority_facts(repo_gate, repo_gate_outcome, &fingerprint, checkpoint_payload))
			.transpose()?;

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
			no_effective_delta_operation_id: no_effective_delta
				.as_ref()
				.map(|(operation_id, _)| operation_id.clone()),
			no_effective_delta_facts: no_effective_delta.map(|(_, facts)| facts),
		})
	}

	fn no_effective_delta_authority_facts(
		&self,
		repo_gate: &ResolvedRepoGate<'_>,
		repo_gate_outcome: &RepoGateCommandOutcome,
		fingerprint: &Option<LoopGuardrailWorktreeFingerprint>,
		checkpoint_payload: Option<&Value>,
	) -> Result<(String, NoEffectiveDeltaFacts)> {
		let lane_id = LaneId::new(self.project.service_id(), &self.issue_run.issue.id)?;
		let lane = self
			.state_store
			.lane(&lane_id)?
			.ok_or_else(|| orchestrator::eyre::eyre!("No-effective-delta lane authority is missing."))?;
		let admitted_base_oid = lane.admitted_base_oid().ok_or_else(|| {
			orchestrator::eyre::eyre!("No-effective-delta admitted base authority is missing.")
		})?;
		let fingerprint = fingerprint.as_ref().ok_or_else(|| {
			orchestrator::eyre::eyre!("No-effective-delta worktree fingerprint is unavailable.")
		})?;
		let patch_set = build_canonical_patch_set(
			&self.issue_run.worktree.path,
			admitted_base_oid,
			&fingerprint.head_sha,
		)
		.map_err(|error| orchestrator::eyre::eyre!("Canonical PatchSet failed: {error}"))?;
		let expected_surface_digest = orchestrator::loop_guardrail_text_hash(
			&serde_json::to_string(&orchestrator::json!({
				"title": self.issue_run.issue.title,
				"description": self.issue_run.issue.description,
			}))?,
		);
		let acceptance_criteria_digest = orchestrator::loop_guardrail_text_hash(
			&format!("acceptance:{}", self.issue_run.issue.description),
		);
		let checkpoint_facts_digest = orchestrator::loop_guardrail_text_hash(
			&serde_json::to_string(&checkpoint_payload.cloned().unwrap_or(Value::Null))?,
		);
		let validation_results_digest = orchestrator::loop_guardrail_text_hash(
			&serde_json::to_string(&orchestrator::json!({
				"profile": repo_gate.profile_name(),
				"canonicalize": repo_gate.canonicalize_commands(),
				"verify": repo_gate.verify_commands(),
				"tracked_rewrites": repo_gate_outcome.tracked_rewrite_decision().map(|value| value.to_json()),
			}))?,
		);
		let name_only_digest = orchestrator::loop_guardrail_text_hash(
			&serde_json::to_string(&acceptance::validation_evidence_changed_surfaces(
				&self.issue_run.worktree.path,
			))?,
		);
		let facts = NoEffectiveDeltaFacts::new(
			admitted_base_oid,
			&patch_set.head_oid_hex(),
			&patch_set.merge_base_oid_hex(),
			&patch_set.digest,
			&name_only_digest,
			&fingerprint.effective_status_hash,
			&expected_surface_digest,
			&acceptance_criteria_digest,
			&checkpoint_facts_digest,
			&validation_results_digest,
			checkpoint_payload.map_or(0, validation_evidence_blocker_count) > 0
				|| self.issue_run.issue.blockers.iter().any(|blocker| {
					!self
						.workflow
						.frontmatter()
						.tracker()
						.terminal_states()
						.iter()
						.any(|state| state == &blocker.state.name)
				}),
		)?;
		let operation_id = format!(
			"no-effective-delta:{}",
			orchestrator::loop_guardrail_text_hash(&format!(
				"{}:{}:{}:{}",
				lane_id.project_key(),
				lane_id.tracker_issue_id(),
				admitted_base_oid,
				acceptance_criteria_digest,
			)),
		);
		Ok((operation_id, facts))
	}
}
