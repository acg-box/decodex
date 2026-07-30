use std::{
	collections::BTreeSet,
	path::{Component, Path},
};

use crate::social_validation::{self, SIGNAL_CONFIDENCE, Value};

pub(in crate::social_validation) fn validate_social_post_claims(
	claims: Option<&Value>,
	source_refs: Option<&Value>,
	evidence_digests: Option<&Value>,
	allow_candidate_lineage: bool,
	errors: &mut Vec<String>,
) {
	let (declared_sources, internal_sources, candidate_lineage) =
		declared_sources(source_refs, errors);
	validate_evidence_digests(evidence_digests, &internal_sources, errors);
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
) -> (BTreeSet<String>, BTreeSet<String>, bool) {
	let Some(source_refs) = source_refs.and_then(Value::as_object) else {
		return (BTreeSet::new(), BTreeSet::new(), false);
	};
	let mut declared = BTreeSet::new();
	let mut internal = BTreeSet::new();
	for field in ["release_deltas", "signals", "upstream_impacts", "upstream_reviews", "urls"] {
		let Some(values) = source_refs.get(field).and_then(Value::as_array) else {
			continue;
		};
		for (index, value) in values.iter().enumerate() {
			let Some(value) = value.as_str() else {
				continue;
			};
			if !declared.insert(value.into()) {
				errors.push(format!("source_refs.{field}[{index}] duplicates another source"));
			}
			if field != "urls" {
				let path = Path::new(value);
				if path.is_absolute()
					|| path.extension().and_then(|extension| extension.to_str()) != Some("json")
					|| path.components().any(|component| !matches!(component, Component::Normal(_)))
				{
					errors.push(format!(
						"source_refs.{field}[{index}] must be a normalized repo-relative JSON path"
					));
				}
				internal.insert(value.into());
			}
		}
	}
	let candidate_lineage = source_refs
		.get("social_candidates")
		.and_then(Value::as_array)
		.is_some_and(|values| values.len() == 1);

	(declared, internal, candidate_lineage)
}

fn validate_evidence_digests(
	evidence_digests: Option<&Value>,
	internal_sources: &BTreeSet<String>,
	errors: &mut Vec<String>,
) {
	let Some(digests) = evidence_digests else {
		if !internal_sources.is_empty() {
			errors.push(
				"evidence_digests must bind every internal source reference to immutable content"
					.into(),
			);
		}

		return;
	};
	let Some(digests) = digests.as_object() else {
		errors.push("evidence_digests must be an object".into());

		return;
	};
	let digest_sources = digests.keys().cloned().collect::<BTreeSet<_>>();
	if &digest_sources != internal_sources {
		errors.push(
			"evidence_digests keys must exactly match the declared internal source references"
				.into(),
		);
	}
	for (reference, digest) in digests {
		if !digest.as_str().is_some_and(is_digest) {
			errors.push(format!(
				"evidence_digests[{reference:?}] must be a lowercase SHA-256 digest"
			));
		}
	}
}

fn is_digest(value: &str) -> bool {
	value.len() == 64
		&& value.bytes().all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}
