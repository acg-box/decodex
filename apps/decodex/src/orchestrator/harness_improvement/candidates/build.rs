use std::collections::BTreeMap;

use crate::orchestrator::harness_improvement::{
	HarnessImprovementCandidateSummary, HarnessLinearProjectionSummary, HarnessOutcomeContract,
	HarnessOutcomeProgram, HarnessOutcomeRecordInput, HarnessOutcomeSignals,
	candidates::{contract, signal},
};

pub(in crate::orchestrator::harness_improvement) fn harness_improvement_candidates(
	input: &HarnessOutcomeRecordInput<'_>,
	contracts: &[HarnessOutcomeContract],
	programs: &[HarnessOutcomeProgram],
	signals: &HarnessOutcomeSignals,
	linear_projection: &HarnessLinearProjectionSummary,
) -> Vec<HarnessImprovementCandidateSummary> {
	let mut candidates = BTreeMap::new();

	contract::push_contract_candidates(&mut candidates, input, contracts, programs);
	signal::push_signal_candidates(&mut candidates, input, signals, linear_projection);

	candidates.into_values().collect()
}
