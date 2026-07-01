use serde::{Deserialize, Serialize};

use crate::{loop_contract::validation, prelude::Result};

/// Research or design source used to produce the contract.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct DecisionResearchProvenance {
	kind: String,
	reference: String,
	summary: String,
}
impl DecisionResearchProvenance {
	pub(crate) fn kind(&self) -> &str {
		&self.kind
	}

	pub(crate) fn reference(&self) -> &str {
		&self.reference
	}

	pub(super) fn validate(&self) -> Result<()> {
		validation::validate_required("decision contract research_provenance.kind", &self.kind)?;
		validation::validate_required(
			"decision contract research_provenance.reference",
			&self.reference,
		)?;

		validation::validate_required(
			"decision contract research_provenance.summary",
			&self.summary,
		)
	}
}

/// Non-authoritative research evidence retained before promotion.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct DecisionResearchEvidence {
	#[serde(default = "validation::default_research_evidence_kind")]
	kind: String,
	claim: String,
	support: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	source_ref: Option<String>,
}
impl DecisionResearchEvidence {
	pub(crate) fn kind(&self) -> &str {
		&self.kind
	}

	pub(crate) fn source_ref(&self) -> Option<&str> {
		self.source_ref.as_deref()
	}

	pub(super) fn validate(&self) -> Result<()> {
		validation::validate_required("decision contract research_evidence.kind", &self.kind)?;
		validation::validate_required("decision contract research_evidence.claim", &self.claim)?;
		validation::validate_required(
			"decision contract research_evidence.support",
			&self.support,
		)?;

		validation::validate_optional(
			"decision contract research_evidence.source_ref",
			self.source_ref.as_deref(),
		)
	}
}

/// Option comparison retained as non-authoritative research context.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct DecisionResearchOption {
	option: String,
	#[serde(default)]
	tradeoffs: Vec<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	decision: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	rejected_reason: Option<String>,
}
#[allow(dead_code)]
impl DecisionResearchOption {
	pub(crate) fn option(&self) -> &str {
		&self.option
	}

	pub(super) fn validate(&self) -> Result<()> {
		validation::validate_required("decision contract research_options.option", &self.option)?;
		validation::validate_string_list(
			"decision contract research_options.tradeoffs",
			&self.tradeoffs,
		)?;
		validation::validate_optional(
			"decision contract research_options.decision",
			self.decision.as_deref(),
		)?;

		validation::validate_optional(
			"decision contract research_options.rejected_reason",
			self.rejected_reason.as_deref(),
		)
	}
}
