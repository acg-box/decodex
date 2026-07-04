use std::collections::BTreeSet;

use crate::orchestrator::harness_improvement::{
	DecisionContractRecord, ExecutionConflictDomain, ExecutionProgramRecord,
	HarnessOutcomeContract, HarnessOutcomeProgram, HarnessOutcomeProgramNode,
	HarnessOutcomeRecordInput, HarnessSourceIntent, Result, StateStore,
};

pub(in crate::orchestrator::harness_improvement) fn harness_contracts_for_issue(
	state_store: &StateStore,
	input: &HarnessOutcomeRecordInput<'_>,
) -> Result<Vec<DecisionContractRecord>> {
	let mut records = Vec::new();
	let mut seen = BTreeSet::new();

	for issue_id in [input.issue_id, input.issue_identifier] {
		for record in state_store.list_decision_contracts_for_issue(input.project_id, issue_id)? {
			let key = record.contract_id().to_owned();

			if seen.insert(key) {
				records.push(record);
			}
		}
	}

	records.sort_by(|left, right| left.contract_id().cmp(right.contract_id()));

	Ok(records)
}

pub(in crate::orchestrator::harness_improvement) fn harness_programs_for_contracts(
	state_store: &StateStore,
	project_id: &str,
	contracts: &[DecisionContractRecord],
) -> Result<Vec<ExecutionProgramRecord>> {
	let mut programs = Vec::new();
	let mut seen = BTreeSet::new();

	for contract in contracts {
		for program in
			state_store.list_execution_programs_for_contract(project_id, contract.contract_id())?
		{
			let key = program.program_id().to_owned();

			if seen.insert(key) {
				programs.push(program);
			}
		}
	}

	programs.sort_by(|left, right| left.program_id().cmp(right.program_id()));

	Ok(programs)
}

pub(super) fn harness_source_intent(record: &DecisionContractRecord) -> HarnessSourceIntent {
	let contract = record.contract();

	HarnessSourceIntent {
		contract_id: contract.contract_id().to_owned(),
		status: record.status().as_str().to_owned(),
		summary: contract.source_intent().summary().to_owned(),
		source_issue_identifier: contract
			.source_intent()
			.source_issue_identifier()
			.map(str::to_owned),
	}
}

pub(super) fn harness_outcome_contract(record: &DecisionContractRecord) -> HarnessOutcomeContract {
	let contract = record.contract();
	let readiness = contract.execution_readiness();
	let links = contract.links();

	HarnessOutcomeContract {
		contract_id: contract.contract_id().to_owned(),
		status: record.status().as_str().to_owned(),
		source_issue_id: record.source_issue_id().map(str::to_owned),
		ready_for_issue_shaping: readiness.ready_for_issue_shaping(),
		missing_decision_count: readiness.missing_decisions().len(),
		generated_issue_ids: links.generated_issue_ids().to_vec(),
		generated_issue_identifiers: links.generated_issue_identifiers().to_vec(),
		execution_program_node_ids: links.execution_program_node_ids().to_vec(),
		conflict_domains: readiness.conflict_domains().to_vec(),
	}
}

pub(super) fn harness_outcome_program(record: &ExecutionProgramRecord) -> HarnessOutcomeProgram {
	let program = record.program();
	let nodes = program
		.nodes()
		.iter()
		.map(|node| {
			let linear_issue = node.linear_issue();

			HarnessOutcomeProgramNode {
				node_id: node.node_id().to_owned(),
				program_stage: node.stage().as_str().to_owned(),
				queue_intent: node.queue_intent().as_str().to_owned(),
				linear_issue_id: linear_issue.map(|issue| issue.issue_id().to_owned()),
				linear_issue_identifier: linear_issue
					.map(|issue| issue.issue_identifier().to_owned()),
				conflict_domains: node
					.conflict_domains()
					.iter()
					.map(harness_conflict_domain_label)
					.collect(),
			}
		})
		.collect::<Vec<_>>();

	HarnessOutcomeProgram {
		program_id: record.program_id().to_owned(),
		source_contract_id: record.source_contract_id().map(str::to_owned),
		node_count: nodes.len(),
		nodes,
	}
}

fn harness_conflict_domain_label(domain: &ExecutionConflictDomain) -> String {
	format!("{}:{}", domain.kind().as_str(), domain.key())
}
