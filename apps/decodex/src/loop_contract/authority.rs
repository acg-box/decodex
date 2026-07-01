use serde::{Deserialize, Serialize};

use crate::{
	loop_contract::{schema::DecisionContractStatus, validation},
	prelude::{Result, eyre},
};

/// Proposed or accepted execution authority carried by the contract.
#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct DecisionAcceptedAuthority {
	#[serde(default)]
	accepted_objectives: Vec<String>,
	#[serde(default)]
	non_goals: Vec<String>,
	#[serde(default)]
	constraints: Vec<String>,
	#[serde(default)]
	assumptions: Vec<String>,
	#[serde(default)]
	objections: Vec<String>,
	#[serde(default)]
	stop_conditions: Vec<String>,
}
#[allow(dead_code)]
impl DecisionAcceptedAuthority {
	pub(crate) fn accepted_objectives(&self) -> &[String] {
		&self.accepted_objectives
	}

	pub(crate) fn non_goals(&self) -> &[String] {
		&self.non_goals
	}

	pub(crate) fn constraints(&self) -> &[String] {
		&self.constraints
	}

	pub(crate) fn assumptions(&self) -> &[String] {
		&self.assumptions
	}

	pub(crate) fn objections(&self) -> &[String] {
		&self.objections
	}

	pub(crate) fn stop_conditions(&self) -> &[String] {
		&self.stop_conditions
	}

	pub(super) fn validate(&self, status: DecisionContractStatus) -> Result<()> {
		if status == DecisionContractStatus::AcceptedPromoted && self.accepted_objectives.is_empty()
		{
			eyre::bail!("Accepted decision contracts must include accepted objectives.");
		}

		validation::validate_string_list(
			"decision contract accepted_objectives",
			&self.accepted_objectives,
		)?;
		validation::validate_string_list("decision contract non_goals", &self.non_goals)?;
		validation::validate_string_list("decision contract constraints", &self.constraints)?;
		validation::validate_string_list("decision contract assumptions", &self.assumptions)?;
		validation::validate_string_list("decision contract objections", &self.objections)?;

		validation::validate_string_list("decision contract stop_conditions", &self.stop_conditions)
	}
}
