use std::{
	collections::BTreeSet,
	path::{Component, Path},
};

use serde_json::{Map, Value};

use crate::{
	prelude::{Result, eyre},
	repo_root,
};

const INTERNAL_EVIDENCE: [(&str, &str); 4] = [
	("upstream_reviews", "upstream_review/v1"),
	("upstream_impacts", "upstream_impact/v1"),
	("signals", "signal_entry/v1"),
	("release_deltas", "release_delta/v1"),
];

pub(crate) fn validate_internal_evidence_files(artifact: &Value) -> Result<()> {
	validate_resolved_claims(artifact)?;
	let root = repo_root()?;
	let refs = artifact
		.get("source_refs")
		.and_then(Value::as_object)
		.ok_or_else(|| eyre::eyre!("social artifact source_refs are required"))?;
	let empty_digests = Map::new();
	let digests =
		artifact.get("evidence_digests").and_then(Value::as_object).unwrap_or(&empty_digests);

	for (field, expected_schema) in INTERNAL_EVIDENCE {
		let Some(references) = refs.get(field).and_then(Value::as_array) else {
			continue;
		};
		for reference in references {
			let reference = reference
				.as_str()
				.ok_or_else(|| eyre::eyre!("social artifact source_refs.{field} is invalid"))?;
			let relative = Path::new(reference);
			if relative.is_absolute()
				|| relative.extension().and_then(|value| value.to_str()) != Some("json")
				|| relative.components().any(|component| !matches!(component, Component::Normal(_)))
			{
				return Err(eyre::eyre!(
					"social artifact source_refs.{field} must contain normalized repo-relative JSON paths"
				));
			}
			let path = root.join(relative);
			crate::require_contained_regular_file(&path, &root).map_err(|error| {
				eyre::eyre!("social artifact evidence {reference} is invalid: {error}")
			})?;
			let (payload, actual_digest) = crate::load_json_with_sha256(&path)?;
			if payload.get("schema").and_then(Value::as_str) != Some(expected_schema) {
				return Err(eyre::eyre!(
					"social artifact evidence {reference} must use schema {expected_schema}"
				));
			}
			if digests.get(reference).and_then(Value::as_str) != Some(&actual_digest) {
				return Err(eyre::eyre!(
					"social artifact evidence {reference} does not match its immutable content digest"
				));
			}
		}
	}

	Ok(())
}

fn validate_resolved_claims(artifact: &Value) -> Result<()> {
	let refs = artifact
		.get("source_refs")
		.and_then(Value::as_object)
		.ok_or_else(|| eyre::eyre!("social artifact source_refs are required"))?;
	let declared = ["release_deltas", "signals", "upstream_impacts", "upstream_reviews", "urls"]
		.into_iter()
		.filter_map(|field| refs.get(field).and_then(Value::as_array))
		.flatten()
		.filter_map(Value::as_str)
		.collect::<BTreeSet<_>>();
	let claims = artifact
		.get("claims")
		.and_then(Value::as_array)
		.ok_or_else(|| eyre::eyre!("social artifact claims are required"))?;
	for (index, claim) in claims.iter().enumerate() {
		let evidence = claim
			.get("evidence")
			.and_then(Value::as_str)
			.ok_or_else(|| eyre::eyre!("social artifact claims[{index}].evidence is invalid"))?;
		if !declared.contains(evidence) {
			return Err(eyre::eyre!(
				"social artifact claims[{index}].evidence does not resolve to a declared source reference"
			));
		}
	}

	Ok(())
}

pub(crate) fn source_refs_with_lineage(
	candidate: &Value,
	candidate_ref: String,
	reservation_ref: Option<String>,
) -> Result<Value> {
	let mut refs = candidate
		.get("source_refs")
		.and_then(Value::as_object)
		.cloned()
		.ok_or_else(|| eyre::eyre!("candidate source_refs are required"))?;
	refs.insert("social_candidates".into(), Value::Array(vec![Value::String(candidate_ref)]));
	if let Some(reservation_ref) = reservation_ref {
		refs.insert("reservations".into(), Value::Array(vec![Value::String(reservation_ref)]));
	}

	Ok(Value::Object(refs))
}

pub(crate) fn evidence_digests_value(candidate: &Value) -> Value {
	candidate.get("evidence_digests").cloned().unwrap_or_else(|| Value::Object(Map::new()))
}
