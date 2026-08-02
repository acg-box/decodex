use std::collections::BTreeSet;

use crate::social_validation::{self, SIGNAL_CONFIDENCE, Value};

pub(in crate::social_validation) fn validate_social_post_claims(
	claims: Option<&Value>,
	source_refs: Option<&Value>,
	evidence_digests: Option<&Value>,
	allow_candidate_lineage: bool,
	errors: &mut Vec<String>,
) {
	let (declared_sources, candidate_lineage) = declared_sources(source_refs, errors);
	validate_empty_evidence_digests(evidence_digests, errors);
	let Some(claims) = social_validation::non_empty_array(claims) else {
		errors.push("claims must be a non-empty list of claim objects".into());
		return;
	};

	for (index, claim) in claims.iter().enumerate() {
		let Some(claim) = claim.as_object() else {
			errors.push(format!("claims[{index}] must be an object"));
			continue;
		};
		social_validation::validate_exact_keys(
			claim,
			&format!("claims[{index}]"),
			&["confidence", "evidence", "text"],
			errors,
		);
		for field in ["text", "evidence"] {
			if !social_validation::is_non_empty_string(claim.get(field)) {
				errors.push(format!("claims[{index}].{field} must be a non-empty string"));
			}
		}
		if !social_validation::matches_one_of(claim.get("confidence"), SIGNAL_CONFIDENCE) {
			errors.push(format!(
				"claims[{index}].confidence must be one of {}",
				social_validation::choices(SIGNAL_CONFIDENCE)
			));
		}
		if let Some(evidence) = claim.get("evidence").and_then(Value::as_str)
			&& !declared_sources.contains(evidence)
			&& !(allow_candidate_lineage && declared_sources.is_empty() && candidate_lineage)
		{
			errors.push(format!(
				"claims[{index}].evidence must exactly match one declared source reference"
			));
		}
	}
}

fn declared_sources(
	source_refs: Option<&Value>,
	errors: &mut Vec<String>,
) -> (BTreeSet<String>, bool) {
	let Some(source_refs) = source_refs.and_then(Value::as_object) else {
		return (BTreeSet::new(), false);
	};
	let mut declared = BTreeSet::new();
	if let Some(values) = source_refs.get("urls").and_then(Value::as_array) {
		for (index, value) in values.iter().enumerate() {
			let Some(value) = value.as_str() else {
				continue;
			};
			if !declared.insert(value.into()) {
				errors.push(format!("source_refs.urls[{index}] duplicates another source"));
			}
		}
	}
	let candidate_lineage = source_refs
		.get("social_candidates")
		.and_then(Value::as_array)
		.is_some_and(|values| values.len() == 1);
	(declared, candidate_lineage)
}

fn validate_empty_evidence_digests(evidence_digests: Option<&Value>, errors: &mut Vec<String>) {
	let Some(digests) = evidence_digests else {
		return;
	};
	if !digests.as_object().is_some_and(serde_json::Map::is_empty) {
		errors.push("evidence_digests must be an empty object".into());
	}
}
