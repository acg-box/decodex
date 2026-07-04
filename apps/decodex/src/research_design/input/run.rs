use serde::{Deserialize, Serialize};

use crate::research_design::input::{
	ResearchDesignOutcome, ResearchEvidenceInput, ResearchOptionInput,
	ResearchPrivateEvidenceRefInput, ResearchProposedIssueInput, ResearchProvenanceInput,
	ResearchPublicProjectionRefInput, ResearchSubworkInput,
};

/// Structured bounded research/design input compiled into a latent Decision Contract.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResearchDesignRunInput {
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::research_design) contract_id: Option<String>,
	pub(in crate::research_design) intent: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::research_design) source_issue_identifier: Option<String>,
	pub(in crate::research_design) outcome: ResearchDesignOutcome,
	#[serde(default)]
	pub(in crate::research_design) provenance: Vec<ResearchProvenanceInput>,
	#[serde(default)]
	pub(in crate::research_design) evidence: Vec<ResearchEvidenceInput>,
	#[serde(default)]
	pub(in crate::research_design) options: Vec<ResearchOptionInput>,
	#[serde(default)]
	pub(in crate::research_design) ai_subwork: Vec<ResearchSubworkInput>,
	#[serde(default)]
	pub(in crate::research_design) objectives: Vec<String>,
	#[serde(default)]
	pub(in crate::research_design) non_goals: Vec<String>,
	#[serde(default)]
	pub(in crate::research_design) constraints: Vec<String>,
	#[serde(default)]
	pub(in crate::research_design) assumptions: Vec<String>,
	#[serde(default)]
	pub(in crate::research_design) objections: Vec<String>,
	#[serde(default)]
	pub(in crate::research_design) unresolved_decisions: Vec<String>,
	#[serde(default)]
	pub(in crate::research_design) evidence_gaps: Vec<String>,
	#[serde(default)]
	pub(in crate::research_design) blockers: Vec<String>,
	#[serde(default)]
	pub(in crate::research_design) stop_conditions: Vec<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::research_design) readiness_summary: Option<String>,
	#[serde(default)]
	pub(in crate::research_design) validation_expectations: Vec<String>,
	#[serde(default)]
	pub(in crate::research_design) risk_notes: Vec<String>,
	#[serde(default)]
	pub(in crate::research_design) proposed_issues: Vec<ResearchProposedIssueInput>,
	#[serde(default)]
	pub(in crate::research_design) promotion_targets: Vec<String>,
	#[serde(default)]
	pub(in crate::research_design) conflict_domains: Vec<String>,
	#[serde(default)]
	pub(in crate::research_design) private_evidence_refs: Vec<ResearchPrivateEvidenceRefInput>,
	#[serde(default)]
	pub(in crate::research_design) public_projection_refs: Vec<ResearchPublicProjectionRefInput>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::research_design) public_summary: Option<String>,
}
impl ResearchDesignRunInput {
	pub(crate) fn from_intent(
		intent: impl Into<String>,
		source_issue_identifier: Option<String>,
		outcome: ResearchDesignOutcome,
	) -> Self {
		Self {
			contract_id: None,
			intent: intent.into(),
			source_issue_identifier,
			outcome,
			provenance: Vec::new(),
			evidence: Vec::new(),
			options: Vec::new(),
			ai_subwork: Vec::new(),
			objectives: Vec::new(),
			non_goals: Vec::new(),
			constraints: Vec::new(),
			assumptions: Vec::new(),
			objections: Vec::new(),
			unresolved_decisions: Vec::new(),
			evidence_gaps: Vec::new(),
			blockers: Vec::new(),
			stop_conditions: Vec::new(),
			readiness_summary: None,
			validation_expectations: Vec::new(),
			risk_notes: Vec::new(),
			proposed_issues: Vec::new(),
			promotion_targets: Vec::new(),
			conflict_domains: Vec::new(),
			private_evidence_refs: Vec::new(),
			public_projection_refs: Vec::new(),
			public_summary: None,
		}
	}

	pub(crate) fn source_issue_identifier(&self) -> Option<&str> {
		self.source_issue_identifier.as_deref()
	}
}
