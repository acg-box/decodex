use serde::{Deserialize, Serialize};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	prelude::{Result, eyre},
	tracker::public_text,
};

/// Immutable accepted project policy authority available to the autonomy runtime.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct AutonomyRuntimePolicyRecord {
	pub(in crate::state) project_id: String,
	pub(in crate::state) policy_id: String,
	pub(in crate::state) policy_version: String,
	pub(in crate::state) objective_id: String,
	pub(in crate::state) objective_version: u64,
	pub(in crate::state) objective_digest: String,
	pub(in crate::state) authority_ref: String,
	pub(in crate::state) accepted_by: String,
	pub(in crate::state) accepted_at: String,
	pub(in crate::state) acceptance_source: String,
	pub(in crate::state) public_non_goals: Vec<String>,
}
#[allow(dead_code)]
impl AutonomyRuntimePolicyRecord {
	#[allow(clippy::too_many_arguments)]
	pub(crate) fn new(
		project_id: impl Into<String>,
		policy_id: impl Into<String>,
		policy_version: impl Into<String>,
		objective_id: impl Into<String>,
		objective_version: u64,
		objective_digest: impl Into<String>,
		authority_ref: impl Into<String>,
		accepted_by: impl Into<String>,
		accepted_at: impl Into<String>,
		acceptance_source: impl Into<String>,
		public_non_goals: Vec<String>,
	) -> Result<Self> {
		let record = Self {
			project_id: project_id.into(),
			policy_id: policy_id.into(),
			policy_version: policy_version.into(),
			objective_id: objective_id.into(),
			objective_version,
			objective_digest: objective_digest.into(),
			authority_ref: authority_ref.into(),
			accepted_by: accepted_by.into(),
			accepted_at: accepted_at.into(),
			acceptance_source: acceptance_source.into(),
			public_non_goals,
		};

		record.validate()?;

		Ok(record)
	}

	pub(crate) fn project_id(&self) -> &str {
		&self.project_id
	}

	pub(crate) fn policy_id(&self) -> &str {
		&self.policy_id
	}

	pub(crate) fn policy_version(&self) -> &str {
		&self.policy_version
	}

	pub(crate) fn objective_id(&self) -> &str {
		&self.objective_id
	}

	pub(crate) fn objective_version(&self) -> u64 {
		self.objective_version
	}

	pub(crate) fn objective_digest(&self) -> &str {
		&self.objective_digest
	}

	pub(crate) fn authority_ref(&self) -> &str {
		&self.authority_ref
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

	pub(crate) fn public_non_goals(&self) -> &[String] {
		&self.public_non_goals
	}

	pub(in crate::state) fn validate(&self) -> Result<()> {
		for (name, value) in [
			("project_id", self.project_id.as_str()),
			("policy_id", self.policy_id.as_str()),
			("policy_version", self.policy_version.as_str()),
			("objective_id", self.objective_id.as_str()),
			("objective_digest", self.objective_digest.as_str()),
			("authority_ref", self.authority_ref.as_str()),
			("accepted_by", self.accepted_by.as_str()),
			("accepted_at", self.accepted_at.as_str()),
			("acceptance_source", self.acceptance_source.as_str()),
		] {
			Self::validate_required_field(name, value)?;
		}

		if self.objective_version == 0 {
			eyre::bail!("Autonomy runtime policy objective_version must be greater than zero.");
		}

		OffsetDateTime::parse(&self.accepted_at, &Rfc3339)
			.map_err(|_| eyre::eyre!("Autonomy runtime policy accepted_at must be RFC3339."))?;

		if self.public_non_goals.is_empty() {
			eyre::bail!("Autonomy runtime policy public_non_goals must not be empty.");
		}

		for non_goal in &self.public_non_goals {
			Self::validate_required_field("public_non_goals entry", non_goal)?;
		}

		public_text::validate_public_text_items("public_non_goals", &self.public_non_goals)
			.map_err(|error| eyre::eyre!(error))?;

		Ok(())
	}

	pub(in crate::state) fn validate_key(
		project_id: &str,
		policy_id: &str,
		policy_version: &str,
	) -> Result<()> {
		Self::validate_required_field("project_id", project_id)?;
		Self::validate_required_field("policy_id", policy_id)?;

		Self::validate_required_field("policy_version", policy_version)
	}

	fn validate_required_field(name: &str, value: &str) -> Result<()> {
		if value.trim().is_empty() {
			eyre::bail!("Autonomy runtime policy {name} must not be empty.");
		}

		Ok(())
	}
}

pub(crate) struct AutonomyRuntimePolicyReceiptInput<'a> {
	pub(crate) project_id: &'a str,
	pub(crate) receipt_id: &'a str,
	pub(crate) principal: &'a str,
	pub(crate) candidate_digest: &'a str,
	pub(crate) candidate: &'a AutonomyRuntimePolicyRecord,
	pub(crate) created_at: &'a str,
	pub(crate) expires_at_unix: i64,
}
