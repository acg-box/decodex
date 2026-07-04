use std::collections::BTreeMap;

use crate::orchestrator::harness_improvement::{
	HarnessImprovementCandidateSummary, HarnessOutcomeContract, HarnessOutcomeProgram,
	HarnessOutcomeRecordInput, candidates::util,
};

pub(in crate::orchestrator::harness_improvement::candidates) fn push_contract_candidates(
	candidates: &mut BTreeMap<String, HarnessImprovementCandidateSummary>,
	input: &HarnessOutcomeRecordInput<'_>,
	contracts: &[HarnessOutcomeContract],
	programs: &[HarnessOutcomeProgram],
) {
	if contracts.is_empty() {
		util::insert_candidate(
			candidates,
			"missing_issue_template_field",
			"contract_provenance_missing",
			&format!("issue:{}", input.issue_identifier),
			0,
			"Add source intent and Decision Contract id/provenance to generated issue briefs.",
		);

		return;
	}

	for contract in contracts {
		if contract.missing_decision_count > 0 {
			util::insert_candidate(
				candidates,
				"underspecified_decision_contract",
				"missing_decisions",
				&format!("decision_contract:{}", contract.contract_id),
				0,
				"Require missing decisions to be resolved before promotion or queueing.",
			);
		}
		if contract.generated_issue_ids.is_empty()
			&& contract.generated_issue_identifiers.is_empty()
		{
			util::insert_candidate(
				candidates,
				"missing_issue_template_field",
				"generated_issue_links_missing",
				&format!("decision_contract:{}", contract.contract_id),
				0,
				"Record generated issue ids or identifiers when research is promoted.",
			);
		}
		if contract.conflict_domains.is_empty() {
			util::insert_candidate(
				candidates,
				"missing_issue_template_field",
				"conflict_domains_missing",
				&format!("decision_contract:{}", contract.contract_id),
				0,
				"Require conflict-domain notes in contracts or generated issue templates.",
			);
		}
	}
	for program in programs.iter().filter(|program| program.node_count == 0) {
		util::insert_candidate(
			candidates,
			"stale_readiness_model",
			"execution_program_has_no_nodes",
			&format!("execution_program:{}", program.program_id),
			0,
			"Regenerate internal Execution Program readiness from the accepted contract.",
		);
	}
}
