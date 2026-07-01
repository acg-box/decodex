use serde::{Deserialize, Serialize};

use crate::prelude::Result;

use super::validation::{validate_optional, validate_string_list};

/// Links from the decision contract to generated execution surfaces.
#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct DecisionContractLinks {
	#[serde(default)]
	pub(super) generated_issue_ids: Vec<String>,
	#[serde(default)]
	pub(super) generated_issue_identifiers: Vec<String>,
	#[serde(default)]
	pub(super) execution_program_node_ids: Vec<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) superseded_by_contract_id: Option<String>,
}
#[allow(dead_code)]
impl DecisionContractLinks {
	pub(crate) fn generated_issue_ids(&self) -> &[String] {
		&self.generated_issue_ids
	}

	pub(crate) fn generated_issue_identifiers(&self) -> &[String] {
		&self.generated_issue_identifiers
	}

	pub(crate) fn execution_program_node_ids(&self) -> &[String] {
		&self.execution_program_node_ids
	}

	pub(crate) fn superseded_by_contract_id(&self) -> Option<&str> {
		self.superseded_by_contract_id.as_deref()
	}

	pub(super) fn validate(&self) -> Result<()> {
		validate_string_list(
			"decision contract links.generated_issue_ids",
			&self.generated_issue_ids,
		)?;
		validate_string_list(
			"decision contract links.generated_issue_identifiers",
			&self.generated_issue_identifiers,
		)?;
		validate_string_list(
			"decision contract links.execution_program_node_ids",
			&self.execution_program_node_ids,
		)?;

		validate_optional(
			"decision contract links.superseded_by_contract_id",
			self.superseded_by_contract_id.as_deref(),
		)
	}
}
