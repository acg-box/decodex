use crate::{
	autonomy_signal::{
		AUTONOMY_SIGNAL_SCHEMA, AutonomySignal,
		fingerprint::{self},
		model,
		types::{AutonomySignalKind, AutonomySignalSourceType},
		validation::{self},
	},
	prelude::{Result, eyre},
};

#[allow(dead_code)]
impl AutonomySignal {
	pub(crate) fn validate(&self) -> Result<()> {
		validation::validate_required("autonomy signal schema", &self.schema)?;
		validation::validate_required("autonomy signal id", &self.id)?;
		validation::validate_required("autonomy signal fingerprint", &self.fingerprint)?;
		validation::validate_required("autonomy signal project_id", &self.project_id)?;
		validation::validate_required("autonomy signal objective_id", &self.objective_id)?;
		validation::validate_required("autonomy signal captured_at", &self.captured_at)?;
		validation::validate_required("autonomy signal summary", &self.summary)?;
		validation::validate_required("autonomy signal created_at", &self.created_at)?;
		validation::validate_nonempty_list("autonomy signal source_refs", &self.source_refs)?;
		validation::validate_nonempty_list("autonomy signal evidence", &self.evidence)?;
		validation::validate_string_list(
			"autonomy signal primary_source_refs",
			&self.primary_source_refs,
		)?;
		validation::validate_string_list("autonomy signal contradictions", &self.contradictions)?;
		validation::validate_string_list("autonomy signal gaps", &self.gaps)?;
		validation::validate_optional_required(
			"autonomy signal issue_id",
			self.issue_id.as_deref(),
		)?;
		validation::validate_optional_required("autonomy signal run_id", self.run_id.as_deref())?;
		validation::validate_optional_required(
			"autonomy signal attempt_id",
			self.attempt_id.as_deref(),
		)?;
		validation::validate_optional_required(
			"autonomy signal head_sha",
			self.head_sha.as_deref(),
		)?;

		if self.schema != AUTONOMY_SIGNAL_SCHEMA {
			eyre::bail!("Autonomy signal `{}` has unsupported schema `{}`.", self.id, self.schema);
		}
		if self.record_version != model::autonomy_signal_record_version() {
			eyre::bail!(
				"Autonomy signal `{}` has unsupported record_version `{}`.",
				self.id,
				self.record_version
			);
		}
		if self.objective_version == 0 {
			eyre::bail!(
				"Autonomy signal `{}` objective_version must be greater than zero.",
				self.id
			);
		}
		if !self.proposal_only {
			eyre::bail!("Autonomy signal `{}` must remain proposal-only evidence.", self.id);
		}

		self.validate_source_specific_rules()?;

		self.validate_fingerprint_identity()
	}

	fn validate_source_specific_rules(&self) -> Result<()> {
		if matches!(
			self.source_type,
			AutonomySignalSourceType::Memory | AutonomySignalSourceType::Report
		) && self.primary_source_refs.is_empty()
		{
			eyre::bail!(
				"Memory/report autonomy signal `{}` requires primary_source_refs.",
				self.id
			);
		}
		if self.kind == AutonomySignalKind::ReviewFeedbackCluster
			|| self.source_type == AutonomySignalSourceType::Review
		{
			let Some(review_evidence) = &self.review_evidence else {
				eyre::bail!(
					"Review-derived autonomy signal `{}` requires review_evidence.",
					self.id
				);
			};

			review_evidence.validate()?;

			let Some(head_sha) = self.head_sha.as_deref() else {
				eyre::bail!("Review-derived autonomy signal `{}` requires head_sha.", self.id);
			};

			if review_evidence.head_sha != head_sha {
				eyre::bail!(
					"Review-derived autonomy signal `{}` head_sha must match review evidence head.",
					self.id
				);
			}
		}

		Ok(())
	}

	fn validate_fingerprint_identity(&self) -> Result<()> {
		let expected = fingerprint::autonomy_signal_fingerprint(self)?;

		if expected != self.fingerprint {
			eyre::bail!(
				"Autonomy signal `{}` fingerprint mismatch: expected `{expected}`.",
				self.id
			);
		}

		let expected_id = fingerprint::autonomy_signal_id(&expected);

		if expected_id != self.id {
			eyre::bail!(
				"Autonomy signal id `{}` does not match fingerprint `{expected}`.",
				self.id
			);
		}

		Ok(())
	}
}
