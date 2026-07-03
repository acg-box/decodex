use std::path::Path;

use serde_json::{Map, Value};

use crate::{SOCIAL_PUBLISH_RESERVATION_SCHEMA, SocialReservePublishRequest};

pub(super) fn social_publish_reservation_payload(
	request: &SocialReservePublishRequest,
	root: &Path,
) -> Value {
	let mut refs = Map::new();

	if !request.candidate_paths.is_empty() {
		refs.insert(
			"social_candidates".into(),
			Value::Array(
				request
					.candidate_paths
					.iter()
					.map(|path| {
						Value::String(crate::path_arg(root, &crate::resolve_against(root, path)))
					})
					.collect(),
			),
		);
	}
	if !request.urls.is_empty() {
		refs.insert(
			"urls".into(),
			Value::Array(request.urls.iter().cloned().map(Value::String).collect()),
		);
	}

	let mut owner = Map::new();

	if let Some(value) = request.automation_id.as_deref().filter(|value| !value.is_empty()) {
		owner.insert("automation_id".into(), Value::String(value.to_owned()));
	}
	if let Some(value) = request.run_id.as_deref().filter(|value| !value.is_empty()) {
		owner.insert("run_id".into(), Value::String(value.to_owned()));
	}
	if let Some(value) = request.branch.as_deref().filter(|value| !value.is_empty()) {
		owner.insert("branch".into(), Value::String(value.to_owned()));
	}

	let mut payload = serde_json::json!({
		"schema": SOCIAL_PUBLISH_RESERVATION_SCHEMA,
		"slug": request.slug,
		"channel": "x",
		"target_account": "decodexspace",
		"controller_account": "hackink",
		"mode": request.mode,
		"status": "active",
		"idempotency_key": request.idempotency_key,
		"reserved_at": request.reserved_at,
		"expires_at": request.expires_at,
		"day": request.day,
		"timezone": request.timezone,
		"candidate_refs": refs,
		"duplicate_keys": request.duplicate_keys,
	});

	if !owner.is_empty() {
		payload["owner"] = Value::Object(owner);
	}

	payload
}
