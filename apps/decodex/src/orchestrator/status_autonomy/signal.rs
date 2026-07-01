use crate::{
	orchestrator::{OperatorAutonomySignalStatus, status_autonomy},
	state::ProjectLoopEvidenceSnapshot,
};

pub(super) fn operator_autonomy_signal_statuses(
	loop_evidence: &ProjectLoopEvidenceSnapshot,
) -> Vec<OperatorAutonomySignalStatus> {
	loop_evidence
		.recent_autonomy_signals(5)
		.into_iter()
		.map(|record| {
			let signal = record.signal();
			let (source_refs, source_refs_redacted) = status_autonomy::public_autonomy_refs(signal.source_refs());
			let (primary_source_refs, primary_source_refs_redacted) =
				status_autonomy::public_autonomy_refs(signal.primary_source_refs());
			let (gaps, gaps_redacted) = status_autonomy::public_status_values(signal.gaps());
			let (contradictions, contradictions_redacted) =
				status_autonomy::public_status_values(signal.contradictions());
			let mut known_gaps = gaps.clone();

			if source_refs.is_empty() {
				known_gaps.push(String::from("source_refs_missing_or_redacted"));
			}
			if source_refs_redacted || primary_source_refs_redacted {
				known_gaps.push(String::from("source_refs_redacted"));
			}
			if gaps_redacted || contradictions_redacted {
				known_gaps.push(String::from("gap_or_contradiction_redacted"));
			}
			if signal.freshness().as_str() != "fresh" {
				known_gaps.push(format!("freshness_{}", signal.freshness().as_str()));
			}

			known_gaps.sort();
			known_gaps.dedup();
			OperatorAutonomySignalStatus {
				signal_id: signal.id().to_owned(),
				objective_id: signal.objective_id().to_owned(),
				objective_version: signal.objective_version(),
				kind: signal.kind().as_str().to_owned(),
				source_type: signal.source_type().as_str().to_owned(),
				source_refs,
				primary_source_refs,
				freshness: signal.freshness().as_str().to_owned(),
				evidence_class: signal.evidence_class().as_str().to_owned(),
				confidence: signal.confidence().as_str().to_owned(),
				privacy: signal.privacy().as_str().to_owned(),
				redaction_level: signal.privacy().as_str().to_owned(),
				completeness: status_autonomy::operator_autonomy_completeness(&known_gaps),
				gaps,
				known_gaps,
				contradictions,
				updated_at: record.updated_at().to_owned(),
			}
		})
		.collect()
}
