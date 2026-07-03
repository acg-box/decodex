use crate::{
	autonomy_proposal::{
		AutonomyProposalChallengeEvidence, AutonomyProposalChallengeInput,
		AutonomyProposalObjectiveLineage, AutonomyProposalRefusal, AutonomyProposalRefusalReason,
		AutonomyProposalSourceSignal,
		validation::{self},
	},
	autonomy_signal::AutonomySignal,
	prelude::{Result, eyre},
};

impl AutonomyProposalObjectiveLineage {
	pub(super) fn validate(&self) -> Result<()> {
		validation::validate_required(
			"autonomy proposal objective lineage.project_id",
			&self.project_id,
		)?;
		validation::validate_required(
			"autonomy proposal objective lineage.objective_id",
			&self.objective_id,
		)?;

		if self.objective_version == 0 {
			eyre::bail!("Autonomy proposal objective lineage version must be greater than zero.");
		}

		validation::validate_optional_required(
			"autonomy proposal objective lineage.objective_state",
			self.objective_state.as_deref(),
		)?;

		validation::validate_optional_required(
			"autonomy proposal objective lineage.objective_summary",
			self.objective_summary.as_deref(),
		)
	}
}

impl AutonomyProposalSourceSignal {
	pub(super) fn from_signal(signal: &AutonomySignal) -> Self {
		Self {
			signal_id: signal.id().to_owned(),
			kind: signal.kind().as_str().to_owned(),
			freshness: signal.freshness().as_str().to_owned(),
			evidence_class: signal.evidence_class().as_str().to_owned(),
			confidence: signal.confidence().as_str().to_owned(),
			gaps: signal.gaps().to_vec(),
			contradictions: signal.contradictions().to_vec(),
		}
	}

	pub(super) fn validate(&self) -> Result<()> {
		validation::validate_required(
			"autonomy proposal source signal.signal_id",
			&self.signal_id,
		)?;
		validation::validate_required("autonomy proposal source signal.kind", &self.kind)?;
		validation::validate_required(
			"autonomy proposal source signal.freshness",
			&self.freshness,
		)?;
		validation::validate_required(
			"autonomy proposal source signal.evidence_class",
			&self.evidence_class,
		)?;
		validation::validate_required(
			"autonomy proposal source signal.confidence",
			&self.confidence,
		)?;
		validation::validate_string_list("autonomy proposal source signal.gaps", &self.gaps)?;

		validation::validate_string_list(
			"autonomy proposal source signal.contradictions",
			&self.contradictions,
		)
	}
}

impl AutonomyProposalRefusal {
	pub(crate) fn reason(&self) -> AutonomyProposalRefusalReason {
		self.reason
	}

	pub(crate) fn detail(&self) -> &str {
		&self.detail
	}

	pub(crate) fn evidence_refs(&self) -> &[String] {
		&self.evidence_refs
	}

	pub(super) fn new(
		reason: AutonomyProposalRefusalReason,
		detail: impl Into<String>,
		evidence_refs: Vec<String>,
	) -> Self {
		Self { reason, detail: detail.into(), evidence_refs }
	}

	pub(super) fn validate(&self) -> Result<()> {
		validation::validate_required("autonomy proposal refusal.detail", &self.detail)?;

		validation::validate_string_list(
			"autonomy proposal refusal.evidence_refs",
			&self.evidence_refs,
		)
	}
}

impl AutonomyProposalChallengeEvidence {
	pub(super) fn from_input(input: AutonomyProposalChallengeInput) -> Result<Self> {
		let evidence = Self {
			source: input.source,
			actor: input.actor,
			summary: input.summary,
			objections: input.objections,
			evidence_refs: input.evidence_refs,
			recorded_at: input.recorded_at,
			acceptance_authority: false,
		};

		evidence.validate()?;

		Ok(evidence)
	}

	pub(super) fn validate(&self) -> Result<()> {
		validation::validate_required("autonomy proposal challenge.actor", &self.actor)?;
		validation::validate_required("autonomy proposal challenge.summary", &self.summary)?;
		validation::validate_required(
			"autonomy proposal challenge.recorded_at",
			&self.recorded_at,
		)?;
		validation::validate_string_list(
			"autonomy proposal challenge.objections",
			&self.objections,
		)?;
		validation::validate_string_list(
			"autonomy proposal challenge.evidence_refs",
			&self.evidence_refs,
		)?;

		if self.acceptance_authority {
			eyre::bail!("Autonomy proposal challenge evidence cannot be acceptance authority.");
		}

		Ok(())
	}
}
