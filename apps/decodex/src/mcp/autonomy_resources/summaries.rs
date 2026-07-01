use serde_json::{self, Value};

use crate::{
	autonomy_objective::AutonomyObjectiveContract,
	autonomy_proposal::AutonomyProposal,
	autonomy_signal::{AutonomySignal, AutonomySignalPrivacy},
	state::{AutonomyProposalRecord, AutonomySignalRecord},
};

pub(in crate::mcp) fn mcp_autonomy_objective_summary(
	objective: &AutonomyObjectiveContract,
	updated_at: Option<&str>,
) -> Value {
	serde_json::json!({
		"objective_id": objective.id(),
		"objective_version": objective.version(),
		"state": objective.state().as_str(),
		"summary": objective.summary(),
		"goals": objective.goals(),
		"non_goals": objective.non_goals(),
		"metrics": objective.metrics(),
		"allowed_surfaces": objective.allowed_surfaces(),
		"allowed_signal_kinds": objective.allowed_signal_kinds(),
		"validation_gates": objective.validation_gates(),
		"review_policy": objective.review_policy(),
		"acceptance_present": objective.acceptance().is_some(),
		"updated_at": updated_at
	})
}

pub(in crate::mcp) fn mcp_autonomy_signal_summary(
	signal: &AutonomySignal,
	updated_at: Option<&str>,
) -> Value {
	let (source_refs, primary_source_refs, source_ref_count, primary_source_ref_count) =
		mcp_autonomy_signal_ref_summary(signal);

	serde_json::json!({
		"signal_id": signal.id(),
		"objective_id": signal.objective_id(),
		"objective_version": signal.objective_version(),
		"kind": signal.kind().as_str(),
		"source_type": signal.source_type().as_str(),
		"source_refs": source_refs,
		"source_ref_count": source_ref_count,
		"primary_source_refs": primary_source_refs,
		"primary_source_ref_count": primary_source_ref_count,
		"freshness": signal.freshness().as_str(),
		"summary": signal.summary(),
		"evidence_class": signal.evidence_class().as_str(),
		"confidence": signal.confidence().as_str(),
		"redaction_level": signal.privacy().as_str(),
		"gaps": signal.gaps(),
		"contradictions": signal.contradictions(),
		"review_evidence_present": signal.review_evidence().is_some(),
		"updated_at": updated_at
	})
}

pub(in crate::mcp) fn mcp_autonomy_proposal_summary(
	proposal: &AutonomyProposal,
	updated_at: Option<&str>,
) -> Value {
	serde_json::json!({
		"proposal_id": proposal.id(),
		"objective_id": proposal.objective_id(),
		"objective_version": proposal.objective_version(),
		"state": proposal.state().as_str(),
		"summary": proposal.summary(),
		"source_family": proposal.source_family(),
		"intended_surface": proposal.intended_surface(),
		"affected_identifiers": proposal.affected_identifiers(),
		"source_signal_ids": proposal.source_signal_ids(),
		"allowed_surfaces": proposal.allowed_surfaces(),
		"validation_gates": proposal.validation_gates(),
		"issue_candidate_count": proposal.issue_candidates().len(),
		"issue_candidates": proposal
			.issue_candidates()
			.iter()
			.map(|candidate| {
				serde_json::json!({
					"key": candidate.key.as_str(),
					"title": candidate.title.as_str(),
					"objective": candidate.objective.as_str(),
					"stage": candidate.stage.as_str(),
					"dependencies": &candidate.dependencies,
					"conflict_domains": &candidate.conflict_domains,
					"acceptance": &candidate.acceptance,
					"validation": &candidate.validation,
					"risk": &candidate.risk,
					"queue_intent": candidate.queue_intent.as_str()
				})
			})
			.collect::<Vec<_>>(),
		"refusal_reasons": proposal
			.refusal_reasons()
			.iter()
			.map(|refusal| refusal.reason().as_str())
			.collect::<Vec<_>>(),
		"refusals": proposal
			.refusal_reasons()
			.iter()
			.map(|refusal| {
				serde_json::json!({
					"reason": refusal.reason().as_str(),
					"detail": refusal.detail(),
					"evidence_refs": refusal.evidence_refs()
				})
			})
			.collect::<Vec<_>>(),
		"gaps": proposal.gaps(),
		"contradictions": proposal.contradictions(),
		"challenge_evidence_count": proposal.challenge_evidence().len(),
		"updated_at": updated_at
	})
}

pub(super) fn mcp_autonomy_evidence_summary(
	signals: &[AutonomySignalRecord],
	proposals: &[AutonomyProposalRecord],
) -> Value {
	serde_json::json!({
		"signal_count": signals.len(),
		"proposal_count": proposals.len(),
		"signal_refs": signals
			.iter()
			.map(|record| {
				serde_json::json!({
					"signal_id": record.signal_id(),
					"kind": record.kind().as_str(),
					"freshness": record.freshness().as_str(),
					"evidence_class": record.evidence_class().as_str(),
					"confidence": record.confidence().as_str(),
					"redaction_level": record.privacy().as_str()
				})
			})
			.collect::<Vec<_>>(),
		"proposal_refs": proposals
			.iter()
			.map(|record| {
				serde_json::json!({
					"proposal_id": record.proposal_id(),
					"state": record.state().as_str(),
					"objective_id": record.objective_id(),
					"objective_version": record.objective_version()
				})
			})
			.collect::<Vec<_>>(),
		"authority_effect": "evidence_summary_only_no_execution_authority"
	})
}

fn mcp_autonomy_signal_ref_summary(signal: &AutonomySignal) -> (Value, Value, usize, usize) {
	let source_ref_count = signal.source_refs().len();
	let primary_source_ref_count = signal.primary_source_refs().len();

	if signal.privacy() == AutonomySignalPrivacy::LocalPrivate {
		return (
			serde_json::json!([]),
			serde_json::json!([]),
			source_ref_count,
			primary_source_ref_count,
		);
	}

	(
		serde_json::json!(signal.source_refs()),
		serde_json::json!(signal.primary_source_refs()),
		source_ref_count,
		primary_source_ref_count,
	)
}
