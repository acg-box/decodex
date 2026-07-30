//! Deterministic selection of one queued subject for source review.

use std::path::Path;

use serde_json::{Map, Value};
use sha2::{Digest as _, Sha256};

use crate::{
	RadarQueueGeneration, RadarReviewNextReport, RadarReviewNextRequest, RadarSelectedSubject,
	RadarSourceRef, UPSTREAM_REVIEW_QUEUE_SCHEMA,
	prelude::{Result, eyre},
};

const REVIEW_NEXT_SCHEMA: &str = "radar_review_next/v1";
const SELECTION_LINEAGE_SCHEMA: &str = "radar_review_selection/v1";
const MAX_REPO_CHARS: usize = 256;
const MAX_SUBJECT_ID_CHARS: usize = 256;
const MAX_TITLE_CHARS: usize = 320;
const MAX_URL_CHARS: usize = 2_048;
const MAX_SUBJECTS: usize = 64;
const MAX_COMMIT_SHAS: usize = 100;

pub(crate) fn review_next(request: &RadarReviewNextRequest) -> Result<RadarReviewNextReport> {
	review_next_with_hook(request, || {})
}

fn review_next_with_hook(
	request: &RadarReviewNextRequest,
	after_selection: impl FnOnce(),
) -> Result<RadarReviewNextReport> {
	if request.max_age_hours == 0 {
		eyre::bail!("source freshness limit must be at least one hour");
	}

	let queue_relative = Path::new(crate::paths::REVIEW_QUEUE_RELATIVE_PATH);
	let cache = crate::private_fs::PrivateCache::open_existing(&request.cache_root)?;
	let lock = cache.lock()?;
	let raw = lock.read(queue_relative)?;
	let queue = parse_current_queue(request, queue_relative, &raw)?;
	let queue_generation = queue_generation(&queue, queue_relative, &raw)?;
	let handled = crate::content_pair::handled_subjects(&lock, &raw)?;
	let handled_state_sha256 = crate::content_pair::handled_state_sha256(&handled)?;
	let candidates = triage_subjects(&queue)?;
	let selected = candidates
		.into_iter()
		.map(|subject| selected_subject(&queue, subject))
		.collect::<Result<Vec<_>>>()?
		.into_iter()
		.find(|(selected, _)| {
			!handled.contains(&crate::content_pair::SubjectLineage {
				repo: selected.repo.clone(),
				subject_kind: selected.subject_kind.clone(),
				subject_id: selected.subject_id.clone(),
				commit_shas: selected.commit_shas.clone(),
			})
		});

	after_selection();

	let Some((selected, source_refs)) = selected else {
		return Ok(empty_report(queue_generation, handled.len(), handled_state_sha256));
	};
	let selection_sha256 = selection_sha256(
		&queue_generation,
		&selected,
		source_refs.as_slice(),
		handled.len(),
		&handled_state_sha256,
	)?;

	Ok(RadarReviewNextReport {
		schema: REVIEW_NEXT_SCHEMA.to_owned(),
		status: "needs_source_review".to_owned(),
		selected: Some(selected),
		queue_generation,
		handled_count: handled.len(),
		handled_state_sha256,
		source_refs,
		selection_sha256: Some(selection_sha256),
	})
}

#[cfg(test)]
pub(crate) fn review_next_with_selection_hook(
	request: &RadarReviewNextRequest,
	after_selection: impl FnOnce(),
) -> Result<RadarReviewNextReport> {
	review_next_with_hook(request, after_selection)
}

fn parse_current_queue(
	request: &RadarReviewNextRequest,
	queue_relative: &Path,
	raw: &[u8],
) -> Result<Value> {
	let queue: Value = serde_json::from_slice(raw)
		.map_err(|error| eyre::eyre!("Review queue contains invalid JSON: {error}"))?;

	validate_generated_artifact("Review queue", UPSTREAM_REVIEW_QUEUE_SCHEMA, &queue)?;
	let mut freshness_errors = Vec::new();

	crate::validate_source_freshness(
		queue_relative,
		&queue,
		request.max_age_hours,
		crate::OffsetDateTime::now_utc(),
		&mut freshness_errors,
	);
	if !freshness_errors.is_empty() {
		eyre::bail!("Review queue freshness failed:\n- {}", freshness_errors.join("\n- "));
	}

	Ok(queue)
}

fn validate_generated_artifact(label: &str, schema: &str, payload: &Value) -> Result<()> {
	crate::validate_expected_schema(payload, schema, label)?;
	let errors = crate::validate_artifact_errors(payload);

	if errors.is_empty() {
		Ok(())
	} else {
		eyre::bail!("{label} validation failed:\n- {}", errors.join("\n- "))
	}
}

fn queue_generation(
	queue: &Value,
	queue_relative: &Path,
	raw: &[u8],
) -> Result<RadarQueueGeneration> {
	let queue = crate::object_value(queue, "review queue")?;
	let source = queue
		.get("source")
		.and_then(Value::as_object)
		.ok_or_else(|| eyre::eyre!("review queue source must be an object"))?;

	Ok(RadarQueueGeneration {
		queue_ref: relative_ref(queue_relative),
		sha256: sha256_hex(raw),
		generated_at: bounded_string(queue, "generated_at", "review queue generated_at", 64)?,
		upstream_head: bounded_string(source, "upstream_head", "review queue upstream head", 64)?,
	})
}

fn triage_subjects(queue: &Value) -> Result<Vec<Value>> {
	let queue = crate::object_value(queue, "review queue")?;
	let subjects = queue
		.get("subjects")
		.and_then(Value::as_array)
		.ok_or_else(|| eyre::eyre!("review queue subjects must be a list"))?;

	if subjects.len() > MAX_SUBJECTS {
		eyre::bail!("review queue exceeds the bounded subject limit");
	}

	Ok(crate::review_queue::sort_queue_subjects(
		subjects.iter().filter(is_triage_candidate).cloned().collect(),
	))
}

fn is_triage_candidate(subject: &&Value) -> bool {
	let Some(subject) = subject.as_object() else {
		return false;
	};
	let priority = crate::string_field(subject, "review_priority");
	let source_state = crate::string_field(subject, "source_state");
	let surfaces = subject.get("surface_hints").and_then(Value::as_array);
	let flags = subject.get("attention_flags").and_then(Value::as_array);
	let has_meaningful_surface = surfaces.is_some_and(|values| {
		values.iter().any(|value| value.as_str().is_some_and(|value| value != "internal_churn"))
	});
	let has_attention = flags.is_some_and(|values| !values.is_empty());

	matches!(priority, Some("critical" | "high" | "normal"))
		&& matches!(source_state, Some("merged" | "commit_only"))
		&& has_meaningful_surface
		&& has_attention
}

fn selected_subject(
	queue: &Value,
	subject: Value,
) -> Result<(RadarSelectedSubject, Vec<RadarSourceRef>)> {
	let queue = crate::object_value(queue, "review queue")?;
	let subject = crate::object_value(&subject, "queue subject")?;
	let repo = bounded_string(queue, "repo", "review queue repo", MAX_REPO_CHARS)?;
	let subject_kind =
		bounded_string(subject, "subject_kind", "queue subject kind", MAX_SUBJECT_ID_CHARS)?;
	let subject_id =
		bounded_string(subject, "subject_id", "queue subject id", MAX_SUBJECT_ID_CHARS)?;
	let title = bounded_string(subject, "title", "queue subject title", MAX_TITLE_CHARS)?;
	let url = bounded_string(subject, "url", "queue subject URL", MAX_URL_CHARS)?;
	let source_state = bounded_string(subject, "source_state", "queue subject source state", 32)?;
	let commit_shas = bounded_git_ids(subject, "commit_shas")?;
	let slug = crate::slugify(&format!("{repo}-{subject_kind}-{subject_id}"));
	let source_kind = if subject_kind == "pr" { "pull_request" } else { "commit" };
	let source_refs =
		vec![RadarSourceRef { kind: source_kind.to_owned(), title: title.clone(), url }];
	let selected = RadarSelectedSubject {
		repo,
		subject_kind,
		subject_id,
		slug,
		title,
		source_state,
		commit_shas,
	};

	Ok((selected, source_refs))
}

fn bounded_string(
	object: &Map<String, Value>,
	field: &str,
	label: &str,
	max_chars: usize,
) -> Result<String> {
	let value = crate::required_string(object, field, label)?;

	if value.chars().count() > max_chars || value.chars().any(char::is_control) {
		eyre::bail!("{label} exceeds its bounded text contract");
	}

	Ok(value.to_owned())
}

fn bounded_git_ids(object: &Map<String, Value>, field: &str) -> Result<Vec<String>> {
	let values = object
		.get(field)
		.and_then(Value::as_array)
		.ok_or_else(|| eyre::eyre!("queue subject {field} must be a list"))?;

	if values.is_empty() || values.len() > MAX_COMMIT_SHAS {
		eyre::bail!("queue subject {field} exceeds its bounded list contract");
	}
	let mut values = values
		.iter()
		.map(|value| {
			value
				.as_str()
				.filter(|value| !value.is_empty())
				.map(ToOwned::to_owned)
				.ok_or_else(|| eyre::eyre!("queue subject {field} must contain strings"))
		})
		.collect::<Result<Vec<_>>>()?;

	if values.iter().any(|value| {
		!matches!(value.len(), 40 | 64)
			|| !value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
	}) {
		eyre::bail!("queue subject {field} must contain lowercase Git object identifiers");
	}
	values.sort();
	values.dedup();

	Ok(values)
}

fn selection_sha256(
	queue_generation: &RadarQueueGeneration,
	selected: &RadarSelectedSubject,
	source_refs: &[RadarSourceRef],
	handled_count: usize,
	handled_state_sha256: &str,
) -> Result<String> {
	let payload = serde_json::json!({
		"schema": SELECTION_LINEAGE_SCHEMA,
		"queue_generation": queue_generation,
		"handled_count": handled_count,
		"handled_state_sha256": handled_state_sha256,
		"selected": selected,
		"source_refs": source_refs,
	});

	Ok(sha256_hex(&serde_json::to_vec(&payload)?))
}

fn sha256_hex(payload: &[u8]) -> String {
	Sha256::digest(payload).iter().map(|byte| format!("{byte:02x}")).collect()
}

fn relative_ref(path: &Path) -> String {
	path.components()
		.filter_map(|component| component.as_os_str().to_str())
		.collect::<Vec<_>>()
		.join("/")
}

fn empty_report(
	queue_generation: RadarQueueGeneration,
	handled_count: usize,
	handled_state_sha256: String,
) -> RadarReviewNextReport {
	RadarReviewNextReport {
		schema: REVIEW_NEXT_SCHEMA.to_owned(),
		status: "no_eligible_item".to_owned(),
		selected: None,
		queue_generation,
		handled_count,
		handled_state_sha256,
		source_refs: Vec::new(),
		selection_sha256: None,
	}
}
