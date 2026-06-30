use serde::{Deserialize, Serialize};

use super::validation::validate_repo_relative_paths;

/// Repo-local early-load context.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowContext {
	read_first: Vec<String>,
}
impl WorkflowContext {
	/// Repository-relative files to load before the broader prompt body.
	pub fn read_first(&self) -> &[String] {
		&self.read_first
	}

	pub(super) fn validate(&self) -> crate::prelude::Result<()> {
		validate_repo_relative_paths("context.read_first", &self.read_first)
	}
}
