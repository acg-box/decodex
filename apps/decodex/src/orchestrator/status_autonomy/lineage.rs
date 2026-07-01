use std::collections::BTreeSet;

use crate::{
	orchestrator::{
		OperatorAutonomyDecisionContractStatus, OperatorAutonomyLineageStatus,
		OperatorAutonomyProgramIntakeStatus, status_autonomy,
		status_autonomy::evidence,
	},
	state::ProjectLoopEvidenceSnapshot,
};

pub(super) fn operator_autonomy_lineage_statuses(
	loop_evidence: &ProjectLoopEvidenceSnapshot,
) -> Vec<OperatorAutonomyLineageStatus> {
	loop_evidence
		.recent_autonomy_proposals(5)
		.into_iter()
		.map(|record| {
			let proposal = record.proposal();
			let contract_records =
				loop_evidence.decision_contracts_for_autonomy_proposal(proposal.id());
			let decision_contracts = contract_records
				.iter()
				.map(|record| OperatorAutonomyDecisionContractStatus {
					contract_id: record.contract_id().to_owned(),
					status: record.status().as_str().to_owned(),
					updated_at: record.updated_at().to_owned(),
					generated_issue_identifiers: record
						.contract()
						.links()
						.generated_issue_identifiers()
						.to_vec(),
				})
				.collect::<Vec<_>>();
			let execution_evidence = evidence::operator_autonomy_execution_evidence_statuses(
				loop_evidence,
				proposal.id(),
				&contract_records,
			);
			let program_intake = decision_contracts
				.iter()
				.flat_map(|contract| {
					loop_evidence
						.program_intake_plans_for_contract(&contract.contract_id)
						.into_iter()
						.map(|plan| OperatorAutonomyProgramIntakeStatus {
							program_id: plan.program_id().to_owned(),
							plan_id: plan.plan_id().to_owned(),
							intake_kind: plan.intake_kind().to_owned(),
							source_contract_id: plan
								.source_contract_id()
								.unwrap_or("none")
								.to_owned(),
							public_summary: status_autonomy::public_or_redacted_status_value(plan.public_summary()),
							updated_at: plan.updated_at().to_owned(),
						})
						.collect::<Vec<_>>()
				})
				.collect::<Vec<_>>();
			let mut known_gaps = Vec::new();

			if proposal.source_signal_ids().is_empty() {
				known_gaps.push(String::from("signal_lineage_missing"));
			}
			if decision_contracts.is_empty() {
				known_gaps.push(String::from("decision_contract_not_materialized"));
			}
			if program_intake.is_empty() {
				known_gaps.push(String::from("program_intake_not_materialized"));
			}
			if !program_intake.is_empty() {
				let evidence_kinds = execution_evidence
					.iter()
					.map(|evidence| evidence.kind.as_str())
					.collect::<BTreeSet<_>>();

				for (kind, gap) in [
					("pr", "pr_evidence_missing"),
					("validation", "validation_evidence_missing"),
					("post_land", "post_land_evidence_missing"),
				] {
					if !evidence_kinds.contains(kind) {
						known_gaps.push(String::from(gap));
					}
				}

				known_gaps.extend(
					execution_evidence
						.iter()
						.flat_map(|evidence| evidence.known_gaps.iter().cloned()),
				);
			}

			let (proposal_gaps, proposal_gaps_redacted) = status_autonomy::public_status_values(proposal.gaps());

			known_gaps.extend(proposal_gaps);

			if proposal_gaps_redacted {
				known_gaps.push(String::from("proposal_gaps_redacted"));
			}

			known_gaps.sort();
			known_gaps.dedup();
			OperatorAutonomyLineageStatus {
				objective_ref: status_autonomy::operator_autonomy_objective_ref(
					proposal.objective_id(),
					proposal.objective_version(),
				),
				signal_ids: proposal.source_signal_ids().to_vec(),
				proposal_id: Some(proposal.id().to_owned()),
				proposal_state: Some(proposal.state().as_str().to_owned()),
				decision_contracts,
				program_intake,
				execution_evidence,
				completeness: status_autonomy::operator_autonomy_completeness(&known_gaps),
				known_gaps,
			}
		})
		.collect()
}
