use crate::{
	prelude::Result,
	research_design::{
		ResearchDesignOutcome, ResearchDesignRunInput,
		input::{
			ResearchEvidenceInput, ResearchOptionInput, ResearchPrivateEvidenceRefInput,
			ResearchProposedIssueInput, ResearchProvenanceInput, ResearchPublicProjectionRefInput,
			ResearchSubworkInput,
		},
		normalized::{self, generated_id, text},
	},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::research_design) struct NormalizedResearchDesignInput {
	pub(in crate::research_design) contract_id: String,
	pub(in crate::research_design) intent: String,
	pub(in crate::research_design) source_issue_identifier: Option<String>,
	pub(in crate::research_design) outcome: ResearchDesignOutcome,
	pub(in crate::research_design) provenance: Vec<ResearchProvenanceInput>,
	pub(in crate::research_design) evidence: Vec<ResearchEvidenceInput>,
	pub(in crate::research_design) options: Vec<ResearchOptionInput>,
	pub(in crate::research_design) ai_subwork: Vec<ResearchSubworkInput>,
	pub(in crate::research_design) objectives: Vec<String>,
	pub(in crate::research_design) non_goals: Vec<String>,
	pub(in crate::research_design) constraints: Vec<String>,
	pub(in crate::research_design) assumptions: Vec<String>,
	pub(in crate::research_design) objections: Vec<String>,
	pub(in crate::research_design) unresolved_decisions: Vec<String>,
	pub(in crate::research_design) evidence_gaps: Vec<String>,
	pub(in crate::research_design) blockers: Vec<String>,
	pub(in crate::research_design) stop_conditions: Vec<String>,
	pub(in crate::research_design) readiness_summary: String,
	pub(in crate::research_design) validation_expectations: Vec<String>,
	pub(in crate::research_design) risk_notes: Vec<String>,
	pub(in crate::research_design) proposed_issues: Vec<ResearchProposedIssueInput>,
	pub(in crate::research_design) promotion_targets: Vec<String>,
	pub(in crate::research_design) conflict_domains: Vec<String>,
	pub(in crate::research_design) private_evidence_refs: Vec<ResearchPrivateEvidenceRefInput>,
	pub(in crate::research_design) public_projection_refs: Vec<ResearchPublicProjectionRefInput>,
	pub(in crate::research_design) public_summary: Option<String>,
}
impl NormalizedResearchDesignInput {
	pub(in crate::research_design) fn new(input: ResearchDesignRunInput) -> Result<Self> {
		let contract_id = match input.contract_id.clone() {
			Some(contract_id) => text::normalize_required_text("contract_id", contract_id)?,
			None => generated_id::generated_contract_id(&input)?,
		};

		Ok(Self {
			contract_id,
			intent: text::normalize_required_text("intent", input.intent)?,
			source_issue_identifier: text::normalize_optional_text(
				"source_issue_identifier",
				input.source_issue_identifier,
			)?,
			outcome: input.outcome,
			provenance: input
				.provenance
				.into_iter()
				.map(ResearchProvenanceInput::normalized)
				.collect::<Result<Vec<_>>>()?,
			evidence: input
				.evidence
				.into_iter()
				.map(ResearchEvidenceInput::normalized)
				.collect::<Result<Vec<_>>>()?,
			options: input
				.options
				.into_iter()
				.map(ResearchOptionInput::normalized)
				.collect::<Result<Vec<_>>>()?,
			ai_subwork: input
				.ai_subwork
				.into_iter()
				.map(ResearchSubworkInput::normalized)
				.collect::<Result<Vec<_>>>()?,
			objectives: text::normalize_text_list("objectives", input.objectives)?,
			non_goals: text::normalize_text_list("non_goals", input.non_goals)?,
			constraints: text::normalize_text_list("constraints", input.constraints)?,
			assumptions: text::normalize_text_list("assumptions", input.assumptions)?,
			objections: text::normalize_text_list("objections", input.objections)?,
			unresolved_decisions: text::normalize_text_list(
				"unresolved_decisions",
				input.unresolved_decisions,
			)?,
			evidence_gaps: text::normalize_text_list("evidence_gaps", input.evidence_gaps)?,
			blockers: text::normalize_text_list("blockers", input.blockers)?,
			stop_conditions: text::normalize_text_list("stop_conditions", input.stop_conditions)?,
			readiness_summary: text::normalize_optional_text(
				"readiness_summary",
				input.readiness_summary,
			)?
			.unwrap_or_else(|| normalized::default_feedback(input.outcome).to_owned()),
			validation_expectations: text::normalize_text_list(
				"validation_expectations",
				input.validation_expectations,
			)?,
			risk_notes: text::normalize_text_list("risk_notes", input.risk_notes)?,
			proposed_issues: input
				.proposed_issues
				.into_iter()
				.map(ResearchProposedIssueInput::normalized)
				.collect::<Result<Vec<_>>>()?,
			promotion_targets: text::normalize_text_list(
				"promotion_targets",
				input.promotion_targets,
			)?,
			conflict_domains: text::normalize_text_list(
				"conflict_domains",
				input.conflict_domains,
			)?,
			private_evidence_refs: input
				.private_evidence_refs
				.into_iter()
				.map(ResearchPrivateEvidenceRefInput::normalized)
				.collect::<Result<Vec<_>>>()?,
			public_projection_refs: input
				.public_projection_refs
				.into_iter()
				.map(ResearchPublicProjectionRefInput::normalized)
				.collect::<Result<Vec<_>>>()?,
			public_summary: text::normalize_optional_text("public_summary", input.public_summary)?,
		})
	}
}
