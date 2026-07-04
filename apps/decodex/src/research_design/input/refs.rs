use serde::{Deserialize, Serialize};

use crate::{prelude::Result, research_design::normalized};

/// Runtime-private evidence pointer retained inside the Decision Contract boundary.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResearchPrivateEvidenceRefInput {
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::research_design) project_id: Option<String>,
	pub(in crate::research_design) issue_id: String,
	pub(in crate::research_design) run_id: String,
	pub(in crate::research_design) attempt_number: i64,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::research_design) record_id: Option<i64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::research_design) event_type: Option<String>,
}
impl ResearchPrivateEvidenceRefInput {
	pub(in crate::research_design) fn normalized(self) -> Result<Self> {
		Ok(Self {
			project_id: normalized::normalize_optional_text(
				"private_evidence_refs.project_id",
				self.project_id,
			)?,
			issue_id: normalized::normalize_required_text(
				"private_evidence_refs.issue_id",
				self.issue_id,
			)?,
			run_id: normalized::normalize_required_text(
				"private_evidence_refs.run_id",
				self.run_id,
			)?,
			attempt_number: self.attempt_number,
			record_id: self.record_id,
			event_type: normalized::normalize_optional_text(
				"private_evidence_refs.event_type",
				self.event_type,
			)?,
		})
	}
}

/// Sparse public projection pointer, such as an issue or summary record.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResearchPublicProjectionRefInput {
	pub(in crate::research_design) surface: String,
	pub(in crate::research_design) reference: String,
	pub(in crate::research_design) summary: String,
}
impl ResearchPublicProjectionRefInput {
	pub(in crate::research_design) fn normalized(self) -> Result<Self> {
		Ok(Self {
			surface: normalized::normalize_required_text(
				"public_projection_refs.surface",
				self.surface,
			)?,
			reference: normalized::normalize_required_text(
				"public_projection_refs.reference",
				self.reference,
			)?,
			summary: normalized::normalize_required_text(
				"public_projection_refs.summary",
				self.summary,
			)?,
		})
	}
}
