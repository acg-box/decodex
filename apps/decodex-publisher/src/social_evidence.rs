use std::collections::BTreeSet;

use serde_json::{Map, Value};

use crate::prelude::{Result, eyre};

pub(crate) fn validate_source_evidence(artifact: &Value) -> Result<()> {
	let refs = artifact
		.get("source_refs")
		.and_then(Value::as_object)
		.ok_or_else(|| eyre::eyre!("social artifact source_refs are required"))?;
	let declared = refs
		.get("urls")
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
		.filter_map(Value::as_str)
		.collect::<BTreeSet<_>>();
	if declared.is_empty() {
		return Err(eyre::eyre!("social artifact requires source_refs.urls"));
	}
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
				"social artifact claims[{index}].evidence does not resolve to a declared source URL"
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

pub(crate) fn evidence_digests_value(_candidate: &Value) -> Value {
	Value::Object(Map::new())
}
