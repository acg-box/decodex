use serde::Deserialize;

use crate::{
	config::validation,
	prelude::{Result, eyre},
};

/// Project-autonomy references from service configuration.
#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectAutonomyConfig {
	#[serde(default)]
	auto_promote: bool,
	#[serde(default)]
	auto_intake: bool,
	runtime_policy: Option<ProjectAutonomyRuntimePolicyConfig>,
}
impl ProjectAutonomyConfig {
	/// Whether accepted runtime policy may promote proposals without another chat turn.
	pub fn auto_promote(&self) -> bool {
		self.auto_promote
	}

	/// Whether accepted runtime policy may enter Program Intake after promotion.
	pub fn auto_intake(&self) -> bool {
		self.auto_intake
	}

	/// References to accepted runtime authority records, when configured.
	pub fn runtime_policy(&self) -> Option<&ProjectAutonomyRuntimePolicyConfig> {
		self.runtime_policy.as_ref()
	}

	pub(super) fn validate(&self) -> Result<()> {
		if self.auto_intake && !self.auto_promote {
			eyre::bail!("`autonomy.auto_intake = true` requires `autonomy.auto_promote = true`.");
		}
		if self.auto_promote && self.runtime_policy.is_none() {
			eyre::bail!(
				"`autonomy.auto_promote = true` requires `[autonomy.runtime_policy]` references."
			);
		}

		if let Some(runtime_policy) = &self.runtime_policy {
			runtime_policy.validate()?;

			if self.auto_intake && runtime_policy.team_issue_identifier().is_none() {
				eyre::bail!(
					"`autonomy.auto_intake = true` requires `autonomy.runtime_policy.team_issue_identifier`."
				);
			}
		}

		Ok(())
	}
}

/// References to accepted Objective Contract and project-policy authority records.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectAutonomyRuntimePolicyConfig {
	accepted_objective_id: String,
	accepted_objective_version: String,
	accepted_policy_id: String,
	accepted_policy_version: String,
	policy_authority_ref: String,
	team_issue_identifier: Option<String>,
}
impl ProjectAutonomyRuntimePolicyConfig {
	/// Accepted runtime Objective Contract id.
	pub fn accepted_objective_id(&self) -> &str {
		&self.accepted_objective_id
	}

	/// Accepted runtime Objective Contract version.
	pub fn accepted_objective_version(&self) -> &str {
		&self.accepted_objective_version
	}

	/// Accepted runtime project-policy id.
	pub fn accepted_policy_id(&self) -> &str {
		&self.accepted_policy_id
	}

	/// Accepted runtime project-policy version.
	pub fn accepted_policy_version(&self) -> &str {
		&self.accepted_policy_version
	}

	/// Runtime authority reference for the accepted project policy record.
	pub fn policy_authority_ref(&self) -> &str {
		&self.policy_authority_ref
	}

	/// Optional tracker anchor required before automatic intake may create issues.
	pub fn team_issue_identifier(&self) -> Option<&str> {
		self.team_issue_identifier.as_deref()
	}

	pub(super) fn validate(&self) -> Result<()> {
		validation::validate_required_config_string(
			"autonomy.runtime_policy.accepted_objective_id",
			&self.accepted_objective_id,
		)?;
		validation::validate_required_config_string(
			"autonomy.runtime_policy.accepted_objective_version",
			&self.accepted_objective_version,
		)?;
		validation::validate_required_config_string(
			"autonomy.runtime_policy.accepted_policy_id",
			&self.accepted_policy_id,
		)?;
		validation::validate_required_config_string(
			"autonomy.runtime_policy.accepted_policy_version",
			&self.accepted_policy_version,
		)?;
		validation::validate_required_config_string(
			"autonomy.runtime_policy.policy_authority_ref",
			&self.policy_authority_ref,
		)?;

		validation::validate_optional_nonempty_string(
			"autonomy.runtime_policy.team_issue_identifier",
			self.team_issue_identifier.as_deref(),
		)
	}
}
