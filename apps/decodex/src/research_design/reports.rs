use serde::Serialize;

use crate::{
	loop_contract::{DecisionContract, DecisionContractStatus, DecisionProposedIssue},
	state::DecisionContractRecord,
};
use crate::research_design::{
	ResearchDesignOutcome,
	normalized::{self, NormalizedResearchDesignInput},
};

/// Compiler report for one persisted research/design run.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct ResearchDesignRunReport {
	pub(crate) outcome: ResearchDesignOutcome,
	pub(crate) contract_id: String,
	pub(crate) contract_status: DecisionContractStatus,
	pub(crate) source_issue_id: Option<String>,
	pub(crate) ready_for_issue_shaping: bool,
	pub(crate) issue_generation_ready_after_promotion: bool,
	pub(crate) execution_authority_granted: bool,
	pub(crate) feedback: String,
	pub(crate) missing_decisions: Vec<String>,
	pub(crate) blockers: Vec<String>,
	pub(crate) proposed_issues: Vec<DecisionProposedIssue>,
	pub(crate) promotion_targets: Vec<String>,
	pub(crate) conflict_domains: Vec<String>,
	pub(crate) private_evidence_ref_count: usize,
	pub(crate) public_projection_ref_count: usize,
}
impl ResearchDesignRunReport {
	pub(super) fn from_compilation(
		input: &NormalizedResearchDesignInput,
		contract: &DecisionContract,
	) -> Self {
		Self {
			outcome: input.outcome,
			contract_id: contract.contract_id().to_owned(),
			contract_status: contract.status(),
			source_issue_id: input.source_issue_identifier.clone(),
			ready_for_issue_shaping: contract.execution_readiness().ready_for_issue_shaping(),
			issue_generation_ready_after_promotion: input.ready_for_issue_shaping(),
			execution_authority_granted: false,
			feedback: normalized::default_feedback(input.outcome).to_owned(),
			missing_decisions: input.missing_decisions(),
			blockers: input.blockers.clone(),
			proposed_issues: contract.execution_readiness().proposed_issues().to_vec(),
			promotion_targets: contract.execution_readiness().promotion_targets().to_vec(),
			conflict_domains: contract.execution_readiness().conflict_domains().to_vec(),
			private_evidence_ref_count: input.private_evidence_refs.len(),
			public_projection_ref_count: input.public_projection_refs.len(),
		}
	}

	pub(super) fn with_record(mut self, record: &DecisionContractRecord) -> Self {
		self.contract_id = record.contract_id().to_owned();
		self.contract_status = record.status();
		self.ready_for_issue_shaping =
			record.contract().execution_readiness().ready_for_issue_shaping();

		self
	}
}

/// Promotion report for an accepted research/design contract.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct ResearchDesignPromotionReport {
	pub(crate) contract_id: String,
	pub(crate) contract_status: DecisionContractStatus,
	pub(crate) execution_authority_granted: bool,
	pub(crate) ready_for_issue_shaping: bool,
}

pub(super) struct ResearchDesignCompilation {
	pub(super) contract: DecisionContract,
	pub(super) report: ResearchDesignRunReport,
}
