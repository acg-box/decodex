//! Stable autonomy signal fingerprinting.

use sha2::{Digest as _, Sha256};

use crate::{
	autonomy_signal::{model::AutonomySignal, review::AutonomySignalReviewEvidence},
	prelude::Result,
};

pub(super) fn autonomy_signal_id(fingerprint: &str) -> String {
	format!("autonomy_signal:{fingerprint}")
}

pub(super) fn autonomy_signal_fingerprint(signal: &AutonomySignal) -> Result<String> {
	autonomy_signal_fingerprint_for_material(
		signal,
		signal.kind.as_str(),
		signal.source_type.as_str(),
	)
}

pub(super) fn legacy_signal_fingerprint_for_material(
	signal: &AutonomySignal,
	kind: &str,
	source_type: &str,
) -> Result<String> {
	autonomy_signal_fingerprint_for_material(signal, kind, source_type)
}

fn autonomy_signal_fingerprint_for_material(
	signal: &AutonomySignal,
	kind: &str,
	source_type: &str,
) -> Result<String> {
	let material = serde_json::json!({
		"schema": signal.schema,
		"record_version": signal.record_version,
		"project_id": signal.project_id,
		"objective_id": signal.objective_id,
		"objective_version": signal.objective_version,
		"kind": kind,
		"source_type": source_type,
		"source_refs": sorted_strings(&signal.source_refs),
		"primary_source_refs": sorted_strings(&signal.primary_source_refs),
		"issue_id": signal.issue_id,
		"run_id": signal.run_id,
		"attempt_id": signal.attempt_id,
		"head_sha": signal.head_sha,
		"freshness": signal.freshness.as_str(),
		"summary": signal.summary,
		"evidence": sorted_strings(&signal.evidence),
		"evidence_class": signal.evidence_class.as_str(),
		"contradictions": sorted_strings(&signal.contradictions),
		"gaps": sorted_strings(&signal.gaps),
		"confidence": signal.confidence.as_str(),
		"privacy": signal.privacy.as_str(),
		"review_evidence": canonical_review_evidence(signal.review_evidence.as_ref()),
		"proposal_only": signal.proposal_only,
	});
	let payload = serde_json::to_vec(&material)?;
	let digest = Sha256::digest(payload);
	let mut hash = String::with_capacity(64);

	for byte in digest {
		hash.push(char::from(b"0123456789abcdef"[(byte >> 4) as usize]));
		hash.push(char::from(b"0123456789abcdef"[(byte & 0x0f) as usize]));
	}

	Ok(hash)
}

fn canonical_review_evidence(evidence: Option<&AutonomySignalReviewEvidence>) -> serde_json::Value {
	let Some(evidence) = evidence else {
		return serde_json::Value::Null;
	};
	let mut finding_routes = evidence
		.finding_routes
		.iter()
		.map(|route| {
			serde_json::json!({
				"route": route.route,
				"finding_source": route.finding_source,
				"finding_index": route.finding_index,
				"summary": route.summary,
				"evidence_refs": sorted_strings(&route.evidence_refs),
			})
		})
		.collect::<Vec<_>>();

	finding_routes.sort_by_key(serde_json::Value::to_string);

	serde_json::json!({
		"review_phase": evidence.review_phase,
		"review_status": evidence.review_status,
		"head_sha": evidence.head_sha,
		"checkpoint_refs": sorted_strings(&evidence.checkpoint_refs),
		"finding_routes": finding_routes,
	})
}

fn sorted_strings(values: &[String]) -> Vec<&str> {
	let mut values = values.iter().map(String::as_str).collect::<Vec<_>>();

	values.sort_unstable();

	values
}
