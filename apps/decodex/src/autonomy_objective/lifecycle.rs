//! Objective lifecycle metadata and states.

mod records;

pub(crate) use records::{
	AutonomyObjectiveAcceptance, AutonomyObjectiveRejection, AutonomyObjectiveSupersession,
};

use serde::{Deserialize, Serialize};

use crate::{
	autonomy_objective::validation,
	prelude::{Result, eyre},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AutonomyObjectiveState {
	Draft,
	Accepted,
	Rejected,
	Superseded,
}
impl AutonomyObjectiveState {
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::Draft => "draft",
			Self::Accepted => "accepted",
			Self::Rejected => "rejected",
			Self::Superseded => "superseded",
		}
	}
}

/// Actor class for objective lifecycle changes.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AutonomyObjectiveActorKind {
	User,
	RuntimePolicy,
}

#[allow(dead_code)]
impl AutonomyObjectiveAcceptance {
	pub(crate) fn new(
		accepted_by: impl Into<String>,
		accepted_by_kind: AutonomyObjectiveActorKind,
		accepted_at: impl Into<String>,
		acceptance_source: impl Into<String>,
	) -> Result<Self> {
		let acceptance = Self {
			accepted_by: accepted_by.into(),
			accepted_by_kind,
			accepted_at: accepted_at.into(),
			acceptance_source: acceptance_source.into(),
		};

		acceptance.validate()?;

		Ok(acceptance)
	}

	pub(crate) fn accepted_by(&self) -> &str {
		&self.accepted_by
	}

	pub(crate) fn accepted_at(&self) -> &str {
		&self.accepted_at
	}

	pub(crate) fn acceptance_source(&self) -> &str {
		&self.acceptance_source
	}

	pub(super) fn validate(&self) -> Result<()> {
		validation::validate_required(
			"autonomy objective acceptance.accepted_by",
			&self.accepted_by,
		)?;
		validation::validate_required(
			"autonomy objective acceptance.accepted_at",
			&self.accepted_at,
		)?;

		validation::validate_required(
			"autonomy objective acceptance.acceptance_source",
			&self.acceptance_source,
		)
	}
}

#[allow(dead_code)]
impl AutonomyObjectiveRejection {
	pub(crate) fn new(
		rejected_by: impl Into<String>,
		rejected_at: impl Into<String>,
		rejection_source: impl Into<String>,
		reason: impl Into<String>,
	) -> Result<Self> {
		let rejection = Self {
			rejected_by: rejected_by.into(),
			rejected_at: rejected_at.into(),
			rejection_source: rejection_source.into(),
			reason: reason.into(),
		};

		rejection.validate()?;

		Ok(rejection)
	}

	pub(crate) fn reason(&self) -> &str {
		&self.reason
	}

	pub(super) fn validate(&self) -> Result<()> {
		validation::validate_required(
			"autonomy objective rejection.rejected_by",
			&self.rejected_by,
		)?;
		validation::validate_required(
			"autonomy objective rejection.rejected_at",
			&self.rejected_at,
		)?;
		validation::validate_required(
			"autonomy objective rejection.rejection_source",
			&self.rejection_source,
		)?;

		validation::validate_required("autonomy objective rejection.reason", &self.reason)
	}
}

#[allow(dead_code)]
impl AutonomyObjectiveSupersession {
	pub(crate) fn new(
		superseded_by_objective_id: impl Into<String>,
		superseded_by_version: u64,
		superseded_by: impl Into<String>,
		superseded_at: impl Into<String>,
		supersession_source: impl Into<String>,
		reason: impl Into<String>,
	) -> Result<Self> {
		let supersession = Self {
			superseded_by_objective_id: superseded_by_objective_id.into(),
			superseded_by_version,
			superseded_by: superseded_by.into(),
			superseded_at: superseded_at.into(),
			supersession_source: supersession_source.into(),
			reason: reason.into(),
		};

		supersession.validate()?;

		Ok(supersession)
	}

	pub(crate) fn superseded_by_objective_id(&self) -> &str {
		&self.superseded_by_objective_id
	}

	pub(crate) fn superseded_by_version(&self) -> u64 {
		self.superseded_by_version
	}

	pub(super) fn validate(&self) -> Result<()> {
		validation::validate_required(
			"autonomy objective supersession.superseded_by_objective_id",
			&self.superseded_by_objective_id,
		)?;

		if self.superseded_by_version == 0 {
			eyre::bail!(
				"Autonomy objective supersession.superseded_by_version must be greater than zero."
			);
		}

		validation::validate_required(
			"autonomy objective supersession.superseded_by",
			&self.superseded_by,
		)?;
		validation::validate_required(
			"autonomy objective supersession.superseded_at",
			&self.superseded_at,
		)?;
		validation::validate_required(
			"autonomy objective supersession.supersession_source",
			&self.supersession_source,
		)?;

		validation::validate_required("autonomy objective supersession.reason", &self.reason)
	}
}
