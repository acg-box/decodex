//! Social publishing reservation generation and conflict checks.

use crate::{
	Map, Path, PathBuf, SOCIAL_POST_SCHEMA, SOCIAL_PUBLISH_RESERVATION_SCHEMA,
	SocialReservePublishReport, SocialReservePublishRequest, Value,
	prelude::{Result, eyre},
};

#[derive(Debug, Default)]
struct SocialPublishStateScan {
	published_count: usize,
	active_reservation_count: usize,
	idempotency_conflict: Option<PathBuf>,
}

pub(crate) fn reserve_social_publish(
	request: &SocialReservePublishRequest,
) -> Result<SocialReservePublishReport> {
	if request.slug.trim().is_empty() {
		return Err(eyre::eyre!("slug is required"));
	}
	if request.idempotency_key.trim().is_empty() {
		return Err(eyre::eyre!("idempotency_key is required"));
	}
	if request.daily_limit == 0 {
		return Err(eyre::eyre!("daily_limit must be positive"));
	}
	if request.candidate_paths.is_empty() && request.urls.is_empty() {
		return Err(eyre::eyre!("at least one candidate path or URL is required"));
	}
	if request.duplicate_keys.is_empty() {
		return Err(eyre::eyre!("at least one duplicate key is required"));
	}

	let root = crate::repo_root()?;
	let out_dir = crate::resolve_against(&root, &request.out_dir);
	let posts_dir = crate::resolve_against(&root, &request.posts_dir);
	let reservation_path =
		out_dir.join(&request.day).join(format!("{}.json", crate::slugify(&request.slug)));
	let scan =
		scan_social_publish_state(&out_dir, &posts_dir, &request.idempotency_key, &request.day)?;

	if scan.idempotency_conflict.is_some() {
		return Err(eyre::eyre!(
			"idempotency_key already has an active reservation or terminal post: {}",
			request.idempotency_key
		));
	}
	if scan.published_count + scan.active_reservation_count >= request.daily_limit {
		return Err(eyre::eyre!(
			"daily publish cap exhausted for {}: published={}, active_reservations={}, limit={}",
			request.day,
			scan.published_count,
			scan.active_reservation_count,
			request.daily_limit
		));
	}

	let payload = social_publish_reservation_payload(request, &root);

	crate::validate_generated_social_artifact(&payload)
		.map_err(|error| eyre::eyre!("generated reservation failed validation: {error}"))?;

	if !request.dry_run {
		crate::write_new_json(&reservation_path, &payload)?;
	}

	Ok(SocialReservePublishReport {
		status: if request.dry_run { "dry_run".into() } else { "reserved".into() },
		path: crate::path_arg(&root, &reservation_path),
		idempotency_key: request.idempotency_key.clone(),
		daily_limit: request.daily_limit,
		published_count: scan.published_count,
		active_reservation_count: scan.active_reservation_count,
	})
}

fn scan_social_publish_state(
	reservations_dir: &Path,
	posts_dir: &Path,
	idempotency_key: &str,
	day: &str,
) -> Result<SocialPublishStateScan> {
	let mut scan = SocialPublishStateScan::default();

	for payload_path in existing_json_files(reservations_dir)? {
		let payload = crate::load_json(&payload_path)?;

		if payload.get("schema").and_then(Value::as_str) != Some(SOCIAL_PUBLISH_RESERVATION_SCHEMA)
		{
			continue;
		}
		if payload.get("status").and_then(Value::as_str) == Some("active") {
			if payload.get("day").and_then(Value::as_str) == Some(day) {
				scan.active_reservation_count += 1;
			}
			if payload.get("idempotency_key").and_then(Value::as_str) == Some(idempotency_key) {
				scan.idempotency_conflict.get_or_insert(payload_path);
			}
		}
	}
	for payload_path in existing_json_files(posts_dir)? {
		let payload = crate::load_json(&payload_path)?;

		if payload.get("schema").and_then(Value::as_str) != Some(SOCIAL_POST_SCHEMA) {
			continue;
		}

		let status = payload.get("status").and_then(Value::as_str);

		if status == Some("published")
			&& payload
				.get("decision")
				.and_then(Value::as_object)
				.and_then(|decision| decision.get("day"))
				.and_then(Value::as_str)
				== Some(day)
		{
			scan.published_count += 1;
		}
		if status != Some("failed")
			&& payload
				.get("decision")
				.and_then(Value::as_object)
				.and_then(|decision| decision.get("idempotency_key"))
				.and_then(Value::as_str)
				== Some(idempotency_key)
		{
			scan.idempotency_conflict.get_or_insert(payload_path);
		}
	}

	Ok(scan)
}

fn existing_json_files(path: &Path) -> Result<Vec<PathBuf>> {
	if !path.exists() {
		return Ok(Vec::new());
	}

	crate::collect_json_files(&[path.to_path_buf()])
}

fn social_publish_reservation_payload(request: &SocialReservePublishRequest, root: &Path) -> Value {
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
