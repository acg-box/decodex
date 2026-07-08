use std::{collections::BTreeSet, path::Path};

use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::{Deserialize, Serialize};

use crate::{
	prelude::{Result, eyre},
	workflow::validation,
};

/// Narrow, repo-owned gate profile selected from changed tracked files.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowGateProfile {
	match_mode: WorkflowGateMatchMode,
	paths: Vec<String>,
	canonicalize_commands: Vec<String>,
	verify_commands: Vec<String>,
}
impl WorkflowGateProfile {
	/// Match mode for the profile.
	pub fn match_mode(&self) -> WorkflowGateMatchMode {
		self.match_mode
	}

	/// Repo-relative path patterns covered by this profile.
	pub fn paths(&self) -> &[String] {
		&self.paths
	}

	/// Canonicalize commands for this profile.
	pub fn canonicalize_commands(&self) -> &[String] {
		&self.canonicalize_commands
	}

	/// Verify commands for this profile.
	pub fn verify_commands(&self) -> &[String] {
		&self.verify_commands
	}

	pub(in crate::workflow) fn validate(&self, profile_name: &str) -> Result<()> {
		if self.paths.is_empty() {
			eyre::bail!("`execution.gate_profiles.{profile_name}.paths` must not be empty.");
		}
		if self.canonicalize_commands.is_empty() && self.verify_commands.is_empty() {
			eyre::bail!(
				"`execution.gate_profiles.{profile_name}` must declare at least one canonicalize or verify command."
			);
		}

		validation::validate_repo_relative_paths(
			&format!("execution.gate_profiles.{profile_name}.paths"),
			&self.paths,
		)?;

		self.compile_path_set(profile_name)?;

		validation::validate_string_entries(
			&format!("execution.gate_profiles.{profile_name}.canonicalize_commands"),
			&self.canonicalize_commands,
		)?;
		validation::validate_string_entries(
			&format!("execution.gate_profiles.{profile_name}.verify_commands"),
			&self.verify_commands,
		)?;

		Ok(())
	}

	pub(in crate::workflow) fn matches_changed_files(
		&self,
		changed_files: &BTreeSet<String>,
	) -> Result<bool> {
		let path_set = self.compile_path_set("runtime")?;

		match self.match_mode {
			WorkflowGateMatchMode::Only =>
				Ok(changed_files.iter().all(|path| path_set.is_match(Path::new(path)))),
		}
	}

	fn compile_path_set(&self, profile_name: &str) -> Result<GlobSet> {
		let mut builder = GlobSetBuilder::new();

		for path in &self.paths {
			let glob = Glob::new(path).map_err(|error| {
				eyre::eyre!(
					"Invalid glob pattern in `execution.gate_profiles.{profile_name}.paths`: `{path}` ({error})"
				)
			})?;

			builder.add(glob);
		}

		builder.build().map_err(|error| {
			eyre::eyre!("Failed to compile `execution.gate_profiles.{profile_name}.paths`: {error}")
		})
	}
}

/// Match semantics for a repo-owned gate profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowGateMatchMode {
	/// The profile applies only when every changed tracked file is covered by its path set.
	Only,
}
