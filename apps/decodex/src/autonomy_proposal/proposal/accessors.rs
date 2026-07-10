use crate::{
	autonomy_proposal::{
		AutonomyProposal, AutonomyProposalChallengeEvidence, AutonomyProposalIssueCandidate,
		AutonomyProposalRefusal, AutonomyProposalRefusalReason, AutonomyProposalState,
	},
	prelude::Result,
};

#[allow(dead_code)]
impl AutonomyProposal {
	pub(crate) fn preserve_challenge_evidence_from(&mut self, existing: &Self) -> Result<()> {
		let mut expected = self.clone();

		expected.challenge_evidence = existing.challenge_evidence.clone();

		if expected != *existing {
			crate::prelude::eyre::bail!(
				"Autonomy proposal `{}` conflicts with its immutable same-id payload.",
				self.id
			);
		}

		self.challenge_evidence = existing.challenge_evidence.clone();

		Ok(())
	}

	pub(crate) fn id(&self) -> &str {
		&self.id
	}

	pub(crate) fn fingerprint(&self) -> &str {
		&self.fingerprint
	}

	pub(crate) fn project_id(&self) -> &str {
		&self.project_id
	}

	pub(crate) fn objective_id(&self) -> &str {
		&self.objective_id
	}

	pub(crate) fn objective_version(&self) -> u64 {
		self.objective_version
	}

	pub(crate) fn state(&self) -> AutonomyProposalState {
		self.state
	}

	pub(crate) fn source_family(&self) -> &str {
		&self.source_family
	}

	pub(crate) fn intended_surface(&self) -> &str {
		&self.intended_surface
	}

	pub(crate) fn affected_identifiers(&self) -> &[String] {
		&self.affected_identifiers
	}

	pub(crate) fn summary(&self) -> &str {
		&self.summary
	}

	pub(crate) fn source_signal_ids(&self) -> &[String] {
		&self.source_signal_ids
	}

	pub(crate) fn allowed_surfaces(&self) -> &[String] {
		&self.allowed_surfaces
	}

	pub(crate) fn validation_gates(&self) -> &[String] {
		&self.validation_gates
	}

	pub(crate) fn issue_candidates(&self) -> &[AutonomyProposalIssueCandidate] {
		&self.issue_candidates
	}

	pub(crate) fn contradictions(&self) -> &[String] {
		&self.contradictions
	}

	pub(crate) fn gaps(&self) -> &[String] {
		&self.gaps
	}

	pub(crate) fn refusal_reasons(&self) -> &[AutonomyProposalRefusal] {
		&self.refusal_reasons
	}

	pub(crate) fn challenge_evidence(&self) -> &[AutonomyProposalChallengeEvidence] {
		&self.challenge_evidence
	}

	pub(crate) fn challenge_requirements(&self) -> &[String] {
		&self.challenge_requirements
	}

	pub(crate) fn review_requirements(&self) -> &[String] {
		&self.review_requirements
	}

	pub(crate) fn rollback_path(&self) -> &str {
		&self.rollback_path
	}

	pub(crate) fn decision_contract_id(&self) -> String {
		format!("autonomy-decision-{}", &self.fingerprint[..32])
	}

	pub(crate) fn has_refusal_reason(&self, reason: AutonomyProposalRefusalReason) -> bool {
		self.refusal_reasons.iter().any(|refusal| refusal.reason == reason)
	}
}

impl AutonomyProposalChallengeEvidence {
	pub(crate) fn actor(&self) -> &str {
		&self.actor
	}

	pub(crate) fn evidence_refs(&self) -> &[String] {
		&self.evidence_refs
	}

	pub(crate) fn objections(&self) -> &[String] {
		&self.objections
	}
}

impl AutonomyProposalIssueCandidate {
	pub(crate) fn key(&self) -> &str {
		&self.key
	}

	pub(crate) fn queue_intent(&self) -> &str {
		&self.queue_intent
	}
}
