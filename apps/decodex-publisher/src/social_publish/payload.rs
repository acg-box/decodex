use std::path::Path;

use serde_json::{Value, json};

use crate::{
	SOCIAL_PUBLISH_RESERVATION_SCHEMA, SocialReservePublishRequest,
	prelude::{Result, eyre},
};

pub(super) fn social_publish_reservation_payload(
	request: &SocialReservePublishRequest,
	root: &Path,
	candidate: &Value,
	candidate_path: &Path,
) -> Result<Value> {
	let decision = candidate
		.get("decision")
		.and_then(Value::as_object)
		.ok_or_else(|| eyre::eyre!("candidate decision is required"))?;
	let slug = required_string(candidate.get("slug"), "candidate slug")?;
	let mode = required_string(candidate.get("mode"), "candidate mode")?;
	let idempotency_key =
		required_string(decision.get("idempotency_key"), "candidate idempotency_key")?;
	let publication_lineage_sha256 = crate::social_record::publication_lineage_sha256(candidate)?;

	Ok(json!({
		"schema": SOCIAL_PUBLISH_RESERVATION_SCHEMA,
		"slug": slug,
		"channel": "x",
		"target_account": "decodexspace",
		"mode": mode,
			"status": "active",
			"idempotency_key": idempotency_key,
			"publication_lineage_sha256": publication_lineage_sha256,
		"reserved_at": request.reserved_at,
		"expires_at": request.expires_at,
		"day": request.day,
		"timezone": request.timezone,
		"candidate_refs": {
			"social_candidates": [crate::path_arg(root, candidate_path)],
		},
		"duplicate_keys": [slug, idempotency_key],
		"owner": {
			"automation_id": "decodex-xurl-publisher",
			"run_id": request.run_id,
		},
	}))
}

fn required_string<'a>(value: Option<&'a Value>, field: &str) -> Result<&'a str> {
	value
		.and_then(Value::as_str)
		.filter(|value| !value.trim().is_empty())
		.ok_or_else(|| eyre::eyre!("{field} is required"))
}
