mod replay;

use std::collections::BTreeSet;

use crate::{
	orchestrator::OperatorAutonomyExecutionEvidenceStatus,
	state::{DecisionContractRecord, ProjectLoopEvidenceSnapshot},
};

pub(crate) fn operator_autonomy_execution_evidence_statuses(
	loop_evidence: &ProjectLoopEvidenceSnapshot,
	proposal_id: &str,
	contracts: &[&DecisionContractRecord],
) -> Vec<OperatorAutonomyExecutionEvidenceStatus> {
	let contract_ids = contracts.iter().map(|record| record.contract_id()).collect::<BTreeSet<_>>();
	let mut evidence = Vec::new();

	for (issue_id, issue_identifier) in operator_autonomy_generated_issue_pairs(contracts) {
		let review_lifecycle_records = loop_evidence.review_lifecycle_records_for_issue(&issue_id);

		for event in loop_evidence.private_events_for_issue(&issue_id) {
			if let Some(status) = replay::operator_autonomy_replay_evidence_status_from_event(
				event,
				proposal_id,
				&contract_ids,
				issue_identifier.as_deref(),
				&review_lifecycle_records,
			) {
				evidence.push(status);
			}
		}
	}

	evidence.sort_by(|left, right| {
		left.kind
			.cmp(&right.kind)
			.then_with(|| left.issue_identifier.cmp(&right.issue_identifier))
			.then_with(|| left.source_refs.cmp(&right.source_refs))
			.then_with(|| {
				replay::operator_autonomy_evidence_completeness_rank(&right.completeness)
					.cmp(&replay::operator_autonomy_evidence_completeness_rank(&left.completeness))
			})
			.then_with(|| right.updated_at.cmp(&left.updated_at))
			.then_with(|| left.summary.cmp(&right.summary))
	});
	evidence.dedup_by(|left, right| {
		left.kind == right.kind
			&& left.issue_identifier == right.issue_identifier
			&& left.source_refs == right.source_refs
	});

	evidence
}

fn operator_autonomy_generated_issue_pairs(
	contracts: &[&DecisionContractRecord],
) -> Vec<(String, Option<String>)> {
	let mut pairs = contracts
		.iter()
		.flat_map(|record| {
			let links = record.contract().links();

			links
				.generated_issue_ids()
				.iter()
				.enumerate()
				.map(|(index, issue_id)| {
					(issue_id.clone(), links.generated_issue_identifiers().get(index).cloned())
				})
				.collect::<Vec<_>>()
		})
		.collect::<Vec<_>>();

	pairs.sort();
	pairs.dedup();

	pairs
}
