use serde::{Deserialize, Serialize};

use crate::{execution_program::validation, prelude::Result};

/// Dependency edge for one program node.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct ExecutionProgramDependency {
	pub(in crate::execution_program) dependency_id: String,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(in crate::execution_program) required_terminal_states: Vec<String>,
}
impl ExecutionProgramDependency {
	/// Build a dependency edge using the registered workflow terminal states.
	pub(crate) fn new(dependency_id: impl Into<String>) -> Result<Self> {
		let dependency =
			Self { dependency_id: dependency_id.into(), required_terminal_states: Vec::new() };

		dependency.validate()?;

		Ok(dependency)
	}

	/// Override the terminal tracker states that satisfy this dependency.
	pub(crate) fn with_required_terminal_states(
		mut self,
		states: impl IntoIterator<Item = impl Into<String>>,
	) -> Result<Self> {
		self.required_terminal_states = states.into_iter().map(Into::into).collect();

		self.validate()?;

		Ok(self)
	}

	/// Dependency node or issue identifier.
	pub(crate) fn dependency_id(&self) -> &str {
		&self.dependency_id
	}

	pub(in crate::execution_program::model) fn validate(&self) -> Result<()> {
		validation::validate_required(
			"execution program dependency.dependency_id",
			&self.dependency_id,
		)?;

		validation::validate_string_list(
			"execution program dependency.required_terminal_states",
			&self.required_terminal_states,
		)
	}
}
