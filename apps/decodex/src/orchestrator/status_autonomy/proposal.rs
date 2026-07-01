use crate::{
	orchestrator::{
		OperatorAutonomyProposalRefusalStatus, OperatorAutonomyProposalStatus, status_autonomy,
	},
	state::ProjectLoopEvidenceSnapshot,
};

pub(super) fn operator_autonomy_proposal_statuses(
	loop_evidence: &ProjectLoopEvidenceSnapshot,
) -> Vec<OperatorAutonomyProposalStatus> {
	loop_evidence
		.recent_autonomy_proposals(5)
		.into_iter()
		.map(|record| {
			let proposal = record.proposal();
			let (source_family, source_family_redacted) =
				status_autonomy::public_status_value(proposal.source_family());
			let (intended_surface, intended_surface_redacted) =
				status_autonomy::public_status_value(proposal.intended_surface());
			let (affected_identifiers, affected_identifiers_redacted) =
				status_autonomy::public_status_values(proposal.affected_identifiers());
			let (gaps, gaps_redacted) = status_autonomy::public_status_values(proposal.gaps());
			let (contradictions, contradictions_redacted) =
				status_autonomy::public_status_values(proposal.contradictions());
			let refusals = proposal
				.refusal_reasons()
				.iter()
				.map(|refusal| {
					let (evidence_refs, _) = status_autonomy::public_autonomy_refs(refusal.evidence_refs());

					OperatorAutonomyProposalRefusalStatus {
						reason: refusal.reason().as_str().to_owned(),
						detail: status_autonomy::public_or_redacted_status_value(refusal.detail()),
						evidence_refs,
					}
				})
				.collect::<Vec<_>>();
			let mut known_gaps = gaps.clone();

			if proposal.source_signal_ids().is_empty() {
				known_gaps.push(String::from("source_signal_ids_missing"));
			}
			if !proposal.refusal_reasons().is_empty() {
				known_gaps.push(String::from("proposal_refused"));
			}
			if source_family_redacted
				|| intended_surface_redacted
				|| affected_identifiers_redacted
				|| gaps_redacted
				|| contradictions_redacted
			{
				known_gaps.push(String::from("proposal_public_fields_redacted"));
			}

			known_gaps.sort();
			known_gaps.dedup();
			OperatorAutonomyProposalStatus {
				proposal_id: proposal.id().to_owned(),
				objective_id: proposal.objective_id().to_owned(),
				objective_version: proposal.objective_version(),
				state: proposal.state().as_str().to_owned(),
				summary: status_autonomy::public_or_redacted_status_value(proposal.summary()),
				source_family,
				intended_surface,
				affected_identifiers,
				source_signal_ids: proposal.source_signal_ids().to_vec(),
				refusal_reasons: proposal
					.refusal_reasons()
					.iter()
					.map(|refusal| refusal.reason().as_str().to_owned())
					.collect(),
				refusals,
				completeness: status_autonomy::operator_autonomy_completeness(&known_gaps),
				known_gaps,
				gaps,
				contradictions,
				challenge_evidence_count: proposal.challenge_evidence().len(),
				updated_at: record.updated_at().to_owned(),
			}
		})
		.collect()
}
