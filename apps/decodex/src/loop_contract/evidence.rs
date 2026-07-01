use serde::{Deserialize, Serialize};

use crate::prelude::{Result, eyre};

use super::validation::{validate_optional, validate_required};

/// Boundary between private runtime evidence and public tracker projection.
#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct DecisionEvidenceBoundary {
	#[serde(default)]
	pub(super) private_evidence_refs: Vec<DecisionPrivateEvidenceRef>,
	#[serde(default)]
	pub(super) public_projection_refs: Vec<DecisionPublicProjectionRef>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) public_summary: Option<String>,
}
#[allow(dead_code)]
impl DecisionEvidenceBoundary {
	pub(crate) fn private_evidence_refs(&self) -> &[DecisionPrivateEvidenceRef] {
		&self.private_evidence_refs
	}

	pub(crate) fn public_projection_refs(&self) -> &[DecisionPublicProjectionRef] {
		&self.public_projection_refs
	}

	pub(super) fn validate(&self) -> Result<()> {
		for evidence_ref in &self.private_evidence_refs {
			evidence_ref.validate()?;
		}
		for projection_ref in &self.public_projection_refs {
			projection_ref.validate()?;
		}

		validate_optional(
			"decision contract evidence_boundary.public_summary",
			self.public_summary.as_deref(),
		)
	}
}

/// Reference to local-only runtime evidence that must not be mirrored to Linear.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct DecisionPrivateEvidenceRef {
	pub(super) project_id: String,
	pub(super) issue_id: String,
	pub(super) run_id: String,
	pub(super) attempt_number: i64,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) record_id: Option<i64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) event_type: Option<String>,
}
impl DecisionPrivateEvidenceRef {
	fn validate(&self) -> Result<()> {
		validate_required("decision contract private_evidence_ref.project_id", &self.project_id)?;
		validate_required("decision contract private_evidence_ref.issue_id", &self.issue_id)?;
		validate_required("decision contract private_evidence_ref.run_id", &self.run_id)?;

		if self.attempt_number < 1 {
			eyre::bail!("Decision contract private evidence attempt_number must be positive.");
		}

		if let Some(record_id) = self.record_id
			&& record_id < 1
		{
			eyre::bail!("Decision contract private evidence record_id must be positive.");
		}

		validate_optional(
			"decision contract private_evidence_ref.event_type",
			self.event_type.as_deref(),
		)
	}
}

/// Reference to a low-frequency public projection such as Linear or a generated issue.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct DecisionPublicProjectionRef {
	pub(super) surface: String,
	pub(super) reference: String,
	pub(super) summary: String,
}
impl DecisionPublicProjectionRef {
	fn validate(&self) -> Result<()> {
		validate_required("decision contract public_projection_ref.surface", &self.surface)?;
		validate_required("decision contract public_projection_ref.reference", &self.reference)?;

		validate_required("decision contract public_projection_ref.summary", &self.summary)
	}
}
