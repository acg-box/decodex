//! Program Intake Plan metadata for execution programs.

use serde::{Deserialize, Serialize};

use crate::{
	execution_program::{
		contract::{self},
		validation::{self},
	},
	loop_contract::DecisionContract,
	prelude::{Result, eyre},
};

/// Stable schema identifier for serialized Program Intake Plans.
pub(crate) const PROGRAM_INTAKE_PLAN_SCHEMA: &str = "decodex.program_intake_plan/1";
/// Stable record version for serialized Program Intake Plans.
pub(crate) const PROGRAM_INTAKE_PLAN_RECORD_VERSION: u16 = 1;

/// Source shape for a Program Intake Plan.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProgramIntakeKind {
	/// Natural-language goal promoted through an accepted Decision Contract.
	GoalIntake,
	/// Operator-supplied batch of normal issue briefs.
	IssueBatchIntake,
}
impl ProgramIntakeKind {
	/// Stable machine-readable intake kind.
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::GoalIntake => "goal_intake",
			Self::IssueBatchIntake => "issue_batch_intake",
		}
	}
}

/// Durable planning metadata for first-class program intake.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct ProgramIntakePlan {
	#[serde(default = "program_intake_plan_schema")]
	schema: String,
	#[serde(default = "program_intake_plan_record_version")]
	record_version: u16,
	plan_id: String,
	pub(super) service_id: String,
	pub(super) intake_kind: ProgramIntakeKind,
	#[serde(skip_serializing_if = "Option::is_none")]
	source_contract_id: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	source_objective_ref: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	source_proposal_id: Option<String>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	source_signal_refs: Vec<String>,
	pub(super) accepted_contract_fingerprint: String,
	public_summary: String,
}
impl ProgramIntakePlan {
	/// Build program-intake metadata for a promoted natural-language goal.
	pub(crate) fn goal_intake(
		plan_id: impl Into<String>,
		service_id: impl Into<String>,
		contract: &DecisionContract,
		accepted_contract_fingerprint: impl Into<String>,
	) -> Result<Self> {
		contract::ensure_accepted_contract(contract)?;

		let public_summary =
			contract.accepted_authority().accepted_objectives().first().cloned().unwrap_or_else(
				|| format!("Accepted Decision Contract `{}`.", contract.contract_id()),
			);
		let plan = Self {
			schema: program_intake_plan_schema(),
			record_version: PROGRAM_INTAKE_PLAN_RECORD_VERSION,
			plan_id: plan_id.into(),
			service_id: service_id.into(),
			intake_kind: ProgramIntakeKind::GoalIntake,
			source_contract_id: Some(contract.contract_id().to_owned()),
			source_objective_ref: contract::decision_contract_provenance_reference(
				contract,
				"autonomy_objective",
			),
			source_proposal_id: contract::decision_contract_provenance_reference(
				contract,
				"autonomy_proposal",
			),
			source_signal_refs: contract::decision_contract_autonomy_signal_refs(contract),
			accepted_contract_fingerprint: accepted_contract_fingerprint.into(),
			public_summary,
		};

		plan.validate()?;

		Ok(plan)
	}

	/// Build program-intake metadata for an accepted issue batch.
	#[allow(dead_code)]
	pub(crate) fn issue_batch_intake(
		plan_id: impl Into<String>,
		service_id: impl Into<String>,
		accepted_contract_fingerprint: impl Into<String>,
		public_summary: impl Into<String>,
	) -> Result<Self> {
		let plan = Self {
			schema: program_intake_plan_schema(),
			record_version: PROGRAM_INTAKE_PLAN_RECORD_VERSION,
			plan_id: plan_id.into(),
			service_id: service_id.into(),
			intake_kind: ProgramIntakeKind::IssueBatchIntake,
			source_contract_id: None,
			source_objective_ref: None,
			source_proposal_id: None,
			source_signal_refs: Vec::new(),
			accepted_contract_fingerprint: accepted_contract_fingerprint.into(),
			public_summary: public_summary.into(),
		};

		plan.validate()?;

		Ok(plan)
	}

	/// Program intake plan id.
	pub(crate) fn plan_id(&self) -> &str {
		&self.plan_id
	}

	/// Service id that owns this intake plan.
	pub(crate) fn service_id(&self) -> &str {
		&self.service_id
	}

	/// Intake source kind.
	pub(crate) fn intake_kind(&self) -> ProgramIntakeKind {
		self.intake_kind
	}

	/// Accepted Decision Contract id for goal intake.
	pub(crate) fn source_contract_id(&self) -> Option<&str> {
		self.source_contract_id.as_deref()
	}

	/// Accepted Objective Contract lineage, when this plan came from autonomy work.
	pub(crate) fn source_objective_ref(&self) -> Option<&str> {
		self.source_objective_ref.as_deref()
	}

	/// Accepted autonomy proposal lineage, when this plan came from autonomy work.
	pub(crate) fn source_proposal_id(&self) -> Option<&str> {
		self.source_proposal_id.as_deref()
	}

	/// Accepted autonomy signal lineage, when this plan came from autonomy work.
	pub(crate) fn source_signal_refs(&self) -> &[String] {
		&self.source_signal_refs
	}

	/// Stable authority fingerprint for this intake boundary.
	pub(crate) fn accepted_contract_fingerprint(&self) -> &str {
		&self.accepted_contract_fingerprint
	}

	/// Public-safe summary suitable for operator readback.
	pub(crate) fn public_summary(&self) -> &str {
		&self.public_summary
	}

	pub(super) fn validate(&self) -> Result<()> {
		validation::validate_required("program intake plan schema", &self.schema)?;
		validation::validate_required("program intake plan plan_id", &self.plan_id)?;
		validation::validate_required("program intake plan service_id", &self.service_id)?;
		validation::validate_required(
			"program intake plan accepted_contract_fingerprint",
			&self.accepted_contract_fingerprint,
		)?;
		validation::validate_required("program intake plan public_summary", &self.public_summary)?;

		if self.schema != PROGRAM_INTAKE_PLAN_SCHEMA {
			eyre::bail!(
				"Program intake plan `{}` has unsupported schema `{}`.",
				self.plan_id,
				self.schema
			);
		}
		if self.record_version != PROGRAM_INTAKE_PLAN_RECORD_VERSION {
			eyre::bail!(
				"Program intake plan `{}` has unsupported record_version `{}`.",
				self.plan_id,
				self.record_version
			);
		}
		if self.intake_kind == ProgramIntakeKind::GoalIntake
			&& self.source_contract_id.as_deref().is_none_or(str::is_empty)
		{
			eyre::bail!("Goal intake plan `{}` must reference a source contract.", self.plan_id);
		}
		if self.intake_kind == ProgramIntakeKind::IssueBatchIntake
			&& self.source_contract_id.as_deref().is_some_and(|id| !id.is_empty())
		{
			eyre::bail!(
				"Issue-batch intake plan `{}` must not reference a source contract.",
				self.plan_id
			);
		}
		if self.intake_kind == ProgramIntakeKind::IssueBatchIntake
			&& self.source_objective_ref.as_deref().is_some_and(|id| !id.is_empty())
		{
			eyre::bail!(
				"Issue-batch intake plan `{}` must not reference autonomy objective lineage.",
				self.plan_id
			);
		}
		if self.intake_kind == ProgramIntakeKind::IssueBatchIntake
			&& self.source_proposal_id.as_deref().is_some_and(|id| !id.is_empty())
		{
			eyre::bail!(
				"Issue-batch intake plan `{}` must not reference autonomy proposal lineage.",
				self.plan_id
			);
		}
		if self.intake_kind == ProgramIntakeKind::IssueBatchIntake
			&& !self.source_signal_refs.is_empty()
		{
			eyre::bail!(
				"Issue-batch intake plan `{}` must not reference autonomy signal lineage.",
				self.plan_id
			);
		}

		validation::validate_optional(
			"program intake plan source_objective_ref",
			self.source_objective_ref.as_deref(),
		)?;
		validation::validate_optional(
			"program intake plan source_proposal_id",
			self.source_proposal_id.as_deref(),
		)?;
		validation::validate_string_list(
			"program intake plan source_signal_refs",
			&self.source_signal_refs,
		)?;

		Ok(())
	}
}

pub(super) fn program_intake_plan_schema() -> String {
	PROGRAM_INTAKE_PLAN_SCHEMA.to_owned()
}

pub(super) fn program_intake_plan_record_version() -> u16 {
	PROGRAM_INTAKE_PLAN_RECORD_VERSION
}
