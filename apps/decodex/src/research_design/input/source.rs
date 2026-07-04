use serde::{Deserialize, Serialize};

use crate::{prelude::Result, research_design::normalized};

/// Research source that contributed to a compiler run.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResearchProvenanceInput {
	pub(in crate::research_design) kind: String,
	pub(in crate::research_design) reference: String,
	pub(in crate::research_design) summary: String,
}
impl ResearchProvenanceInput {
	pub(in crate::research_design) fn normalized(self) -> Result<Self> {
		Ok(Self {
			kind: normalized::normalize_required_text("provenance.kind", self.kind)?,
			reference: normalized::normalize_required_text("provenance.reference", self.reference)?,
			summary: normalized::normalize_required_text("provenance.summary", self.summary)?,
		})
	}
}

/// Evidence claim retained as research context, not execution authority.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResearchEvidenceInput {
	#[serde(default = "default_evidence_kind")]
	pub(in crate::research_design) kind: String,
	pub(in crate::research_design) claim: String,
	pub(in crate::research_design) support: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::research_design) source_ref: Option<String>,
}
impl ResearchEvidenceInput {
	pub(in crate::research_design) fn normalized(self) -> Result<Self> {
		Ok(Self {
			kind: normalized::normalize_required_text("evidence.kind", self.kind)?,
			claim: normalized::normalize_required_text("evidence.claim", self.claim)?,
			support: normalized::normalize_required_text("evidence.support", self.support)?,
			source_ref: normalized::normalize_optional_text(
				"evidence.source_ref",
				self.source_ref,
			)?,
		})
	}
}

/// Candidate option considered during bounded research/design.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResearchOptionInput {
	pub(in crate::research_design) option: String,
	#[serde(default)]
	pub(in crate::research_design) tradeoffs: Vec<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::research_design) decision: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::research_design) rejected_reason: Option<String>,
}
impl ResearchOptionInput {
	pub(in crate::research_design) fn normalized(self) -> Result<Self> {
		Ok(Self {
			option: normalized::normalize_required_text("options.option", self.option)?,
			tradeoffs: normalized::normalize_text_list("options.tradeoffs", self.tradeoffs)?,
			decision: normalized::normalize_optional_text("options.decision", self.decision)?,
			rejected_reason: normalized::normalize_optional_text(
				"options.rejected_reason",
				self.rejected_reason,
			)?,
		})
	}
}

/// AI-owned subwork folded back into the main coherent contract.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResearchSubworkInput {
	pub(in crate::research_design) worker_kind: String,
	pub(in crate::research_design) objective: String,
	pub(in crate::research_design) outcome: String,
	#[serde(default)]
	pub(in crate::research_design) evidence_refs: Vec<String>,
}
impl ResearchSubworkInput {
	pub(in crate::research_design) fn normalized(self) -> Result<Self> {
		Ok(Self {
			worker_kind: normalized::normalize_required_text(
				"ai_subwork.worker_kind",
				self.worker_kind,
			)?,
			objective: normalized::normalize_required_text("ai_subwork.objective", self.objective)?,
			outcome: normalized::normalize_required_text("ai_subwork.outcome", self.outcome)?,
			evidence_refs: normalized::normalize_text_list(
				"ai_subwork.evidence_refs",
				self.evidence_refs,
			)?,
		})
	}

	pub(in crate::research_design) fn summary(&self) -> String {
		if self.evidence_refs.is_empty() {
			self.outcome.clone()
		} else {
			format!("{} Evidence refs: {}.", self.outcome, self.evidence_refs.join(", "))
		}
	}
}

fn default_evidence_kind() -> String {
	String::from("unspecified")
}
