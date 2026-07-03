use crate::{
	autonomy_objective::{
		AUTONOMY_OBJECTIVE_SCHEMA, AutonomyObjectiveContract, AutonomyObjectiveState,
		AutonomyObjectiveSupersession, contract,
		validation::{self},
	},
	prelude::{Result, eyre},
};

#[allow(dead_code)]
impl AutonomyObjectiveContract {
	pub(crate) fn validate(&self) -> Result<()> {
		validation::validate_required("autonomy objective schema", &self.schema)?;
		validation::validate_required("autonomy objective project_id", &self.project_id)?;
		validation::validate_required("autonomy objective id", &self.id)?;
		validation::validate_required("autonomy objective summary", &self.summary)?;
		validation::validate_required("autonomy objective review_policy", &self.review_policy)?;
		validation::validate_required("autonomy objective memory_policy", &self.memory_policy)?;
		validation::validate_required("autonomy objective report_policy", &self.report_policy)?;

		if self.schema != AUTONOMY_OBJECTIVE_SCHEMA {
			eyre::bail!(
				"Autonomy objective `{}` has unsupported schema `{}`.",
				self.id,
				self.schema
			);
		}
		if self.record_version != contract::autonomy_objective_record_version() {
			eyre::bail!(
				"Autonomy objective `{}` has unsupported record_version `{}`.",
				self.id,
				self.record_version
			);
		}
		if self.version == 0 {
			eyre::bail!("Autonomy objective `{}` version must be greater than zero.", self.id);
		}

		validation::validate_string_list("autonomy objective goals", &self.goals)?;
		validation::validate_string_list("autonomy objective non_goals", &self.non_goals)?;
		validation::validate_string_list("autonomy objective metrics", &self.metrics)?;
		validation::validate_string_list(
			"autonomy objective allowed_surfaces",
			&self.allowed_surfaces,
		)?;
		validation::validate_string_list(
			"autonomy objective allowed_signal_kinds",
			&self.allowed_signal_kinds,
		)?;
		validation::validate_string_list(
			"autonomy objective validation_gates",
			&self.validation_gates,
		)?;

		self.validate_lifecycle_shape()?;
		self.validate_lifecycle_records()?;

		Ok(())
	}

	fn validate_lifecycle_shape(&self) -> Result<()> {
		match self.state {
			AutonomyObjectiveState::Draft => self.validate_draft_lifecycle_shape(),
			AutonomyObjectiveState::Accepted => self.validate_accepted_lifecycle_shape(),
			AutonomyObjectiveState::Rejected => self.validate_rejected_lifecycle_shape(),
			AutonomyObjectiveState::Superseded => self.validate_superseded_lifecycle_shape(),
		}
	}

	fn validate_lifecycle_records(&self) -> Result<()> {
		if let Some(acceptance) = &self.acceptance {
			acceptance.validate()?;
		}
		if let Some(rejection) = &self.rejection {
			rejection.validate()?;
		}
		if let Some(supersession) = &self.supersession {
			supersession.validate()?;
			self.validate_supersession_target(supersession)?;
		}

		Ok(())
	}

	fn validate_draft_lifecycle_shape(&self) -> Result<()> {
		if self.acceptance.is_some() || self.rejection.is_some() || self.supersession.is_some() {
			eyre::bail!(
				"Draft autonomy objective `{}` must not carry lifecycle provenance.",
				self.id
			);
		}

		Ok(())
	}

	fn validate_accepted_lifecycle_shape(&self) -> Result<()> {
		if self.acceptance.is_none() {
			eyre::bail!("Accepted autonomy objective `{}` must include acceptance.", self.id);
		}
		if self.rejection.is_some() || self.supersession.is_some() {
			eyre::bail!(
				"Accepted autonomy objective `{}` must not carry rejection or supersession.",
				self.id
			);
		}

		self.validate_complete_authority_body()
	}

	fn validate_rejected_lifecycle_shape(&self) -> Result<()> {
		if self.rejection.is_none() {
			eyre::bail!("Rejected autonomy objective `{}` must include rejection.", self.id);
		}
		if self.acceptance.is_some() || self.supersession.is_some() {
			eyre::bail!(
				"Rejected autonomy objective `{}` must not carry acceptance or supersession.",
				self.id
			);
		}

		Ok(())
	}

	fn validate_superseded_lifecycle_shape(&self) -> Result<()> {
		if self.supersession.is_none() {
			eyre::bail!("Superseded autonomy objective `{}` must include supersession.", self.id);
		}
		if self.rejection.is_some() {
			eyre::bail!("Superseded autonomy objective `{}` must not carry rejection.", self.id);
		}

		Ok(())
	}

	fn validate_supersession_target(
		&self,
		supersession: &AutonomyObjectiveSupersession,
	) -> Result<()> {
		if supersession.superseded_by_objective_id() == self.id
			&& supersession.superseded_by_version() <= self.version
		{
			eyre::bail!(
				"Autonomy objective `{}` version {} cannot be superseded by same-objective version {}.",
				self.id,
				self.version,
				supersession.superseded_by_version()
			);
		}

		Ok(())
	}

	fn validate_complete_authority_body(&self) -> Result<()> {
		validation::validate_nonempty_list("accepted autonomy objective goals", &self.goals)?;
		validation::validate_nonempty_list(
			"accepted autonomy objective non_goals",
			&self.non_goals,
		)?;
		validation::validate_nonempty_list("accepted autonomy objective metrics", &self.metrics)?;
		validation::validate_nonempty_list(
			"accepted autonomy objective allowed_surfaces",
			&self.allowed_surfaces,
		)?;
		validation::validate_nonempty_list(
			"accepted autonomy objective allowed_signal_kinds",
			&self.allowed_signal_kinds,
		)?;

		validation::validate_nonempty_list(
			"accepted autonomy objective validation_gates",
			&self.validation_gates,
		)
	}
}
