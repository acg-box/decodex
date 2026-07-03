use crate::{
	autonomy_proposal::{
		AUTONOMY_PROPOSAL_RECORD_VERSION, AUTONOMY_PROPOSAL_SCHEMA, AutonomyProposal,
		AutonomyProposalState, validation,
	},
	prelude::{Result, eyre},
};

#[allow(dead_code)]
impl AutonomyProposal {
	pub(crate) fn validate(&self) -> Result<()> {
		self.validate_required_fields()?;
		self.validate_static_invariants()?;
		self.validate_objective_lineage()?;
		self.validate_collections()?;
		self.validate_embedded_records()?;
		self.validate_nested_records()?;

		self.validate_fingerprint_identity()
	}

	fn validate_required_fields(&self) -> Result<()> {
		validation::validate_required("autonomy proposal schema", &self.schema)?;
		validation::validate_required("autonomy proposal id", &self.id)?;
		validation::validate_required("autonomy proposal fingerprint", &self.fingerprint)?;
		validation::validate_required("autonomy proposal project_id", &self.project_id)?;
		validation::validate_required("autonomy proposal objective_id", &self.objective_id)?;
		validation::validate_required("autonomy proposal source_family", &self.source_family)?;
		validation::validate_required(
			"autonomy proposal intended_surface",
			&self.intended_surface,
		)?;
		validation::validate_required("autonomy proposal summary", &self.summary)?;
		validation::validate_required("autonomy proposal rollback_path", &self.rollback_path)?;
		validation::validate_required("autonomy proposal created_at", &self.created_at)?;

		Ok(())
	}

	fn validate_static_invariants(&self) -> Result<()> {
		if self.schema != AUTONOMY_PROPOSAL_SCHEMA {
			eyre::bail!(
				"Autonomy proposal `{}` has unsupported schema `{}`.",
				self.id,
				self.schema
			);
		}
		if self.record_version != AUTONOMY_PROPOSAL_RECORD_VERSION {
			eyre::bail!(
				"Autonomy proposal `{}` has unsupported record_version `{}`.",
				self.id,
				self.record_version
			);
		}
		if self.objective_version == 0 {
			eyre::bail!(
				"Autonomy proposal `{}` objective_version must be greater than zero.",
				self.id
			);
		}
		if !self.dry_run || !self.non_executable {
			eyre::bail!(
				"Autonomy proposal `{}` must remain non-executable dry-run evidence.",
				self.id
			);
		}
		if self.state == AutonomyProposalState::AcceptedPromoted {
			eyre::bail!(
				"Autonomy proposal `{}` cannot claim accepted_promoted in schema version {} without explicit Decision Contract promotion provenance.",
				self.id,
				self.record_version
			);
		}

		Ok(())
	}

	fn validate_objective_lineage(&self) -> Result<()> {
		if self.objective_lineage.project_id != self.project_id
			|| self.objective_lineage.objective_id != self.objective_id
			|| self.objective_lineage.objective_version != self.objective_version
		{
			eyre::bail!(
				"Autonomy proposal `{}` objective lineage must match proposal key.",
				self.id
			);
		}

		self.objective_lineage.validate()
	}

	fn validate_collections(&self) -> Result<()> {
		validation::validate_sorted_unique(
			"autonomy proposal source_signal_ids",
			&self.source_signal_ids,
		)?;
		validation::validate_sorted_unique(
			"autonomy proposal affected_identifiers",
			&self.affected_identifiers,
		)?;
		validation::validate_string_list(
			"autonomy proposal allowed_surfaces",
			&self.allowed_surfaces,
		)?;
		validation::validate_string_list(
			"autonomy proposal validation_gates",
			&self.validation_gates,
		)?;
		validation::validate_string_list("autonomy proposal goals", &self.goals)?;
		validation::validate_string_list("autonomy proposal metrics", &self.metrics)?;
		validation::validate_string_list("autonomy proposal non_goals", &self.non_goals)?;
		validation::validate_string_list(
			"autonomy proposal review_requirements",
			&self.review_requirements,
		)?;
		validation::validate_sorted_unique(
			"autonomy proposal challenge_requirements",
			&self.challenge_requirements,
		)?;
		validation::validate_sorted_unique(
			"autonomy proposal rejected_alternatives",
			&self.rejected_alternatives,
		)?;
		validation::validate_sorted_unique(
			"autonomy proposal contradictions",
			&self.contradictions,
		)?;
		validation::validate_sorted_unique("autonomy proposal gaps", &self.gaps)?;

		Ok(())
	}

	fn validate_embedded_records(&self) -> Result<()> {
		let signal_ids_from_refs =
			self.source_signals.iter().map(|signal| signal.signal_id.clone()).collect::<Vec<_>>();

		if signal_ids_from_refs != self.source_signal_ids {
			eyre::bail!(
				"Autonomy proposal `{}` source_signal_ids must match source_signals.",
				self.id
			);
		}

		Ok(())
	}

	fn validate_nested_records(&self) -> Result<()> {
		for signal in &self.source_signals {
			signal.validate()?;
		}
		for refusal in &self.refusal_reasons {
			refusal.validate()?;
		}
		for challenge in &self.challenge_evidence {
			challenge.validate()?;
		}

		Ok(())
	}

	fn validate_fingerprint_identity(&self) -> Result<()> {
		let expected = validation::autonomy_proposal_fingerprint(self)?;

		if expected != self.fingerprint {
			eyre::bail!(
				"Autonomy proposal `{}` fingerprint mismatch: expected `{expected}`.",
				self.id
			);
		}

		let expected_id = validation::autonomy_proposal_id(&expected);

		if expected_id != self.id {
			eyre::bail!(
				"Autonomy proposal id `{}` does not match fingerprint `{expected}`.",
				self.id
			);
		}

		Ok(())
	}
}
