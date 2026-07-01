//! Objective Contract payload and lifecycle transitions.

use serde::{Deserialize, Serialize};

use crate::prelude::{Result, eyre};

use super::{
	lifecycle::{
		AutonomyObjectiveAcceptance, AutonomyObjectiveRejection, AutonomyObjectiveState,
		AutonomyObjectiveSupersession,
	},
	validation::{validate_nonempty_list, validate_required, validate_string_list},
};

pub(crate) const AUTONOMY_OBJECTIVE_SCHEMA: &str = "decodex.autonomy_objective/1";
pub(crate) const AUTONOMY_OBJECTIVE_RECORD_VERSION: u16 = 1;

/// Versioned project-level Objective Contract payload.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AutonomyObjectiveContract {
	#[serde(default = "autonomy_objective_schema")]
	schema: String,
	#[serde(default = "autonomy_objective_record_version")]
	record_version: u16,
	project_id: String,
	id: String,
	version: u64,
	state: AutonomyObjectiveState,
	summary: String,
	#[serde(default)]
	goals: Vec<String>,
	#[serde(default)]
	non_goals: Vec<String>,
	#[serde(default)]
	metrics: Vec<String>,
	#[serde(default)]
	allowed_surfaces: Vec<String>,
	#[serde(default)]
	allowed_signal_kinds: Vec<String>,
	#[serde(default)]
	validation_gates: Vec<String>,
	review_policy: String,
	memory_policy: String,
	report_policy: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	acceptance: Option<AutonomyObjectiveAcceptance>,
	#[serde(skip_serializing_if = "Option::is_none")]
	rejection: Option<AutonomyObjectiveRejection>,
	#[serde(skip_serializing_if = "Option::is_none")]
	supersession: Option<AutonomyObjectiveSupersession>,
}
#[allow(dead_code)]
impl AutonomyObjectiveContract {
	pub(crate) fn project_id(&self) -> &str {
		&self.project_id
	}

	pub(crate) fn id(&self) -> &str {
		&self.id
	}

	pub(crate) fn version(&self) -> u64 {
		self.version
	}

	pub(crate) fn state(&self) -> AutonomyObjectiveState {
		self.state
	}

	pub(crate) fn summary(&self) -> &str {
		&self.summary
	}

	pub(crate) fn goals(&self) -> &[String] {
		&self.goals
	}

	pub(crate) fn non_goals(&self) -> &[String] {
		&self.non_goals
	}

	pub(crate) fn metrics(&self) -> &[String] {
		&self.metrics
	}

	pub(crate) fn allowed_surfaces(&self) -> &[String] {
		&self.allowed_surfaces
	}

	pub(crate) fn allowed_signal_kinds(&self) -> &[String] {
		&self.allowed_signal_kinds
	}

	pub(crate) fn validation_gates(&self) -> &[String] {
		&self.validation_gates
	}

	pub(crate) fn review_policy(&self) -> &str {
		&self.review_policy
	}

	pub(crate) fn acceptance(&self) -> Option<&AutonomyObjectiveAcceptance> {
		self.acceptance.as_ref()
	}

	pub(crate) fn rejection(&self) -> Option<&AutonomyObjectiveRejection> {
		self.rejection.as_ref()
	}

	pub(crate) fn supersession(&self) -> Option<&AutonomyObjectiveSupersession> {
		self.supersession.as_ref()
	}

	pub(crate) fn validate(&self) -> Result<()> {
		validate_required("autonomy objective schema", &self.schema)?;
		validate_required("autonomy objective project_id", &self.project_id)?;
		validate_required("autonomy objective id", &self.id)?;
		validate_required("autonomy objective summary", &self.summary)?;
		validate_required("autonomy objective review_policy", &self.review_policy)?;
		validate_required("autonomy objective memory_policy", &self.memory_policy)?;
		validate_required("autonomy objective report_policy", &self.report_policy)?;

		if self.schema != AUTONOMY_OBJECTIVE_SCHEMA {
			eyre::bail!(
				"Autonomy objective `{}` has unsupported schema `{}`.",
				self.id,
				self.schema
			);
		}
		if self.record_version != AUTONOMY_OBJECTIVE_RECORD_VERSION {
			eyre::bail!(
				"Autonomy objective `{}` has unsupported record_version `{}`.",
				self.id,
				self.record_version
			);
		}
		if self.version == 0 {
			eyre::bail!("Autonomy objective `{}` version must be greater than zero.", self.id);
		}

		validate_string_list("autonomy objective goals", &self.goals)?;
		validate_string_list("autonomy objective non_goals", &self.non_goals)?;
		validate_string_list("autonomy objective metrics", &self.metrics)?;
		validate_string_list("autonomy objective allowed_surfaces", &self.allowed_surfaces)?;
		validate_string_list(
			"autonomy objective allowed_signal_kinds",
			&self.allowed_signal_kinds,
		)?;
		validate_string_list("autonomy objective validation_gates", &self.validation_gates)?;

		match self.state {
			AutonomyObjectiveState::Draft => {
				if self.acceptance.is_some()
					|| self.rejection.is_some()
					|| self.supersession.is_some()
				{
					eyre::bail!(
						"Draft autonomy objective `{}` must not carry lifecycle provenance.",
						self.id
					);
				}
			},
			AutonomyObjectiveState::Accepted => {
				if self.acceptance.is_none() {
					eyre::bail!(
						"Accepted autonomy objective `{}` must include acceptance.",
						self.id
					);
				}
				if self.rejection.is_some() || self.supersession.is_some() {
					eyre::bail!(
						"Accepted autonomy objective `{}` must not carry rejection or supersession.",
						self.id
					);
				}

				self.validate_complete_authority_body()?;
			},
			AutonomyObjectiveState::Rejected => {
				if self.rejection.is_none() {
					eyre::bail!(
						"Rejected autonomy objective `{}` must include rejection.",
						self.id
					);
				}
				if self.acceptance.is_some() || self.supersession.is_some() {
					eyre::bail!(
						"Rejected autonomy objective `{}` must not carry acceptance or supersession.",
						self.id
					);
				}
			},
			AutonomyObjectiveState::Superseded => {
				if self.supersession.is_none() {
					eyre::bail!(
						"Superseded autonomy objective `{}` must include supersession.",
						self.id
					);
				}
				if self.rejection.is_some() {
					eyre::bail!(
						"Superseded autonomy objective `{}` must not carry rejection.",
						self.id
					);
				}
			},
		}

		if let Some(acceptance) = &self.acceptance {
			acceptance.validate()?;
		}
		if let Some(rejection) = &self.rejection {
			rejection.validate()?;
		}
		if let Some(supersession) = &self.supersession {
			supersession.validate()?;

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
		}

		Ok(())
	}

	pub(crate) fn accept(&mut self, acceptance: AutonomyObjectiveAcceptance) -> Result<()> {
		match self.state {
			AutonomyObjectiveState::Draft => {},
			AutonomyObjectiveState::Accepted => {
				eyre::bail!(
					"Autonomy objective `{}` version {} is already accepted.",
					self.id,
					self.version
				);
			},
			AutonomyObjectiveState::Rejected | AutonomyObjectiveState::Superseded => {
				eyre::bail!(
					"Autonomy objective `{}` version {} cannot be accepted from state `{}`.",
					self.id,
					self.version,
					self.state.as_str()
				);
			},
		}

		acceptance.validate()?;

		let mut candidate = self.clone();

		candidate.state = AutonomyObjectiveState::Accepted;
		candidate.acceptance = Some(acceptance);
		candidate.rejection = None;
		candidate.supersession = None;

		candidate.validate()?;

		*self = candidate;

		Ok(())
	}

	pub(crate) fn reject(&mut self, rejection: AutonomyObjectiveRejection) -> Result<()> {
		if self.state != AutonomyObjectiveState::Draft {
			eyre::bail!(
				"Autonomy objective `{}` version {} can only be rejected from draft state.",
				self.id,
				self.version
			);
		}

		rejection.validate()?;

		let mut candidate = self.clone();

		candidate.state = AutonomyObjectiveState::Rejected;
		candidate.acceptance = None;
		candidate.rejection = Some(rejection);
		candidate.supersession = None;

		candidate.validate()?;

		*self = candidate;

		Ok(())
	}

	pub(crate) fn supersede(&mut self, supersession: AutonomyObjectiveSupersession) -> Result<()> {
		if matches!(
			self.state,
			AutonomyObjectiveState::Rejected | AutonomyObjectiveState::Superseded
		) {
			eyre::bail!(
				"Autonomy objective `{}` version {} cannot be superseded from state `{}`.",
				self.id,
				self.version,
				self.state.as_str()
			);
		}

		supersession.validate()?;

		let mut candidate = self.clone();

		candidate.state = AutonomyObjectiveState::Superseded;
		candidate.rejection = None;
		candidate.supersession = Some(supersession);

		candidate.validate()?;

		*self = candidate;

		Ok(())
	}

	fn validate_complete_authority_body(&self) -> Result<()> {
		validate_nonempty_list("accepted autonomy objective goals", &self.goals)?;
		validate_nonempty_list("accepted autonomy objective non_goals", &self.non_goals)?;
		validate_nonempty_list("accepted autonomy objective metrics", &self.metrics)?;
		validate_nonempty_list(
			"accepted autonomy objective allowed_surfaces",
			&self.allowed_surfaces,
		)?;
		validate_nonempty_list(
			"accepted autonomy objective allowed_signal_kinds",
			&self.allowed_signal_kinds,
		)?;

		validate_nonempty_list(
			"accepted autonomy objective validation_gates",
			&self.validation_gates,
		)
	}
}

fn autonomy_objective_schema() -> String {
	AUTONOMY_OBJECTIVE_SCHEMA.to_owned()
}

const fn autonomy_objective_record_version() -> u16 {
	AUTONOMY_OBJECTIVE_RECORD_VERSION
}
