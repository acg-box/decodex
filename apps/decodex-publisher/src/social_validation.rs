//! Decodex social artifact validation.

use std::{collections::BTreeMap, path::Path};

use serde_json::{Map, Value};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	SOCIAL_CANDIDATE_SCHEMA, SOCIAL_POST_SCHEMA, SOCIAL_PUBLISH_RESERVATION_SCHEMA, path_arg,
	repo_root,
};

const SIGNAL_CONFIDENCE: &[&str] = &["confirmed", "likely", "weak"];
const SOCIAL_BLOCK_REASONS: &[&str] =
	&["daily_cap_exceeded", "duplicate", "insufficient_evidence", "policy_block"];
const SOCIAL_POST_LIFECYCLE_STATES: &[&str] = &[
	"deleted_by_operator",
	"live",
	"superseded_failed_attempt",
	"superseded_published",
	"superseded_text_only",
];
const SOCIAL_POST_MODES: &[&str] = &[
	"operator_impact",
	"practical_explainer",
	"release_pulse",
	"release_rollup",
	"thread",
	"watch_note",
];
const SOCIAL_POST_PRIORITIES: &[&str] = &["critical", "high", "low", "normal"];
const SOCIAL_POST_STATUSES: &[&str] = &["blocked", "failed", "published", "skipped"];
const SOCIAL_POST_WORTHINESS: &[&str] = &["block", "publish", "skip"];
const SOCIAL_PUBLISH_RESERVATION_STATUSES: &[&str] = &["active", "canceled", "consumed", "expired"];

pub(crate) struct SocialArtifactValidation {
	pub(crate) errors: Vec<String>,
}

#[derive(Debug)]
pub(crate) struct SocialValidationState {
	active_reservation_idempotency_keys: BTreeMap<String, String>,
	terminal_post_idempotency_keys: BTreeMap<String, String>,
}
impl SocialValidationState {
	pub(crate) fn new() -> Self {
		Self {
			active_reservation_idempotency_keys: BTreeMap::new(),
			terminal_post_idempotency_keys: BTreeMap::new(),
		}
	}
}

pub(crate) fn validate_social_artifact_for_path(
	_path: &Path,
	payload: &Value,
) -> SocialArtifactValidation {
	validate_social_artifact(payload)
}

pub(crate) fn validate_social_artifact(payload: &Value) -> SocialArtifactValidation {
	let Some(entry) = payload.as_object() else {
		return SocialArtifactValidation { errors: vec!["artifact must be an object".into()] };
	};
	let mut errors = Vec::new();

	match string_field(entry, "schema") {
		Some(SOCIAL_CANDIDATE_SCHEMA) => validate_social_candidate(entry, &mut errors),
		Some(SOCIAL_POST_SCHEMA) => validate_social_post(entry, &mut errors),
		Some(SOCIAL_PUBLISH_RESERVATION_SCHEMA) =>
			validate_social_publish_reservation(entry, &mut errors),
		Some(_) | None => errors.push(format!(
			"schema must be one of {}",
			choices(&[
				SOCIAL_CANDIDATE_SCHEMA,
				SOCIAL_POST_SCHEMA,
				SOCIAL_PUBLISH_RESERVATION_SCHEMA
			])
		)),
	}

	SocialArtifactValidation { errors }
}

pub(crate) fn validate_social_cross_file_constraints(
	path: &Path,
	payload: &Value,
	state: &mut SocialValidationState,
	errors: &mut Vec<String>,
) {
	let root = repo_root().ok();
	let display_path = root
		.as_deref()
		.map_or_else(|| path.to_string_lossy().replace('\\', "/"), |root| path_arg(root, path));

	match payload.get("schema").and_then(Value::as_str) {
		Some(SOCIAL_POST_SCHEMA) => {
			let status = payload.get("status").and_then(Value::as_str);

			if !matches!(status, Some("published" | "blocked")) {
				return;
			}

			let Some(key) = payload
				.get("decision")
				.and_then(Value::as_object)
				.and_then(|decision| decision.get("idempotency_key"))
				.and_then(Value::as_str)
			else {
				return;
			};

			if let Some(existing) =
				state.terminal_post_idempotency_keys.insert(key.to_owned(), display_path.clone())
			{
				errors.push(format!(
					"{display_path}: duplicate terminal social_post idempotency_key {key:?} also used by {existing}"
				));
			}
			if let Some(existing) = state.active_reservation_idempotency_keys.get(key) {
				errors.push(format!(
					"{display_path}: terminal social_post idempotency_key {key:?} conflicts with active reservation {existing}"
				));
			}
		},
		Some(SOCIAL_PUBLISH_RESERVATION_SCHEMA) => {
			if payload.get("status").and_then(Value::as_str) != Some("active") {
				return;
			}

			let Some(key) = payload.get("idempotency_key").and_then(Value::as_str) else {
				return;
			};

			if let Some(existing) = state.terminal_post_idempotency_keys.get(key) {
				errors.push(format!(
					"{display_path}: active social_publish_reservation idempotency_key {key:?} conflicts with terminal social_post {existing}"
				));
			}
			if let Some(existing) = state
				.active_reservation_idempotency_keys
				.insert(key.to_owned(), display_path.clone())
			{
				errors.push(format!(
					"{display_path}: duplicate active social_publish_reservation idempotency_key {key:?} also used by {existing}"
				));
			}
		},
		_ => {},
	}
}

fn validate_social_candidate(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	for field in ["slug", "repo", "audience"] {
		if !is_non_empty_string(entry.get(field)) {
			errors.push(format!("{field} must be a non-empty string"));
		}
	}

	if string_field(entry, "repo").is_some_and(|repo| !repo.contains('/')) {
		errors.push("repo must be owner/name".into());
	}
	if string_field(entry, "channel") != Some("x") {
		errors.push("channel must be x".into());
	}
	if string_field(entry, "target_account") != Some("decodexspace") {
		errors.push("target_account must be decodexspace".into());
	}
	if !matches_one_of(entry.get("mode"), SOCIAL_POST_MODES) {
		errors.push(format!("mode must be one of {}", choices(SOCIAL_POST_MODES)));
	}
	if !matches_one_of(entry.get("priority"), SOCIAL_POST_PRIORITIES) {
		errors.push(format!("priority must be one of {}", choices(SOCIAL_POST_PRIORITIES)));
	}

	validate_social_post_text(entry.get("candidate_text"), errors);
	validate_social_candidate_source_refs(entry.get("source_refs"), errors);
	validate_non_empty_string_list(entry.get("evidence_notes"), "evidence_notes", errors);
	validate_social_post_claims(entry.get("claims"), errors);
	validate_social_candidate_decision(entry.get("decision"), errors);

	for field in ["caveats", "media_refs", "next_steps"] {
		validate_optional_string_list(entry.get(field), field, errors);
	}
}

fn validate_social_candidate_source_refs(refs: Option<&Value>, errors: &mut Vec<String>) {
	let Some(refs) = refs.and_then(Value::as_object) else {
		errors.push("source_refs must be an object".into());

		return;
	};
	let has_refs = ["upstream_reviews", "upstream_impacts", "signals", "release_deltas", "urls"]
		.iter()
		.any(|field| refs.get(*field).is_some_and(|value| !is_empty_or_missing_array(Some(value))));

	if !has_refs {
		errors.push(
			"source_refs must include upstream_reviews, upstream_impacts, signals, release_deltas, or urls"
				.into(),
		);
	}

	let uses_radar_inputs = ["upstream_reviews", "release_deltas"]
		.iter()
		.any(|field| non_empty_array(refs.get(*field)).is_some());

	if uses_radar_inputs && non_empty_array(refs.get("upstream_impacts")).is_none() {
		errors.push(
			"source_refs.upstream_impacts must include the shared upstream_impact/v1 handoff for Radar-derived social candidates"
				.into(),
		);
	}
	if refs.get("urls").is_some_and(|urls| !is_https_string_array(urls)) {
		errors.push("source_refs.urls must be a list of https URLs".into());
	}

	for field in ["upstream_reviews", "upstream_impacts", "signals", "release_deltas"] {
		validate_optional_string_list(refs.get(field), &format!("source_refs.{field}"), errors);
	}
}

fn validate_social_candidate_decision(decision: Option<&Value>, errors: &mut Vec<String>) {
	let Some(decision) = decision.and_then(Value::as_object) else {
		errors.push("decision must be an object".into());

		return;
	};

	if !matches_one_of(decision.get("worthiness"), &["defer", "publish", "skip"]) {
		errors.push("decision.worthiness must be one of ['defer', 'publish', 'skip']".into());
	}

	for field in ["reason", "idempotency_key"] {
		if !is_non_empty_string(decision.get(field)) {
			errors.push(format!("decision.{field} must be a non-empty string"));
		}
	}
}

fn validate_social_publish_reservation(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	for field in ["slug", "idempotency_key", "reserved_at", "expires_at", "day", "timezone"] {
		if !is_non_empty_string(entry.get(field)) {
			errors.push(format!("{field} must be a non-empty string"));
		}
	}

	validate_social_publish_reservation_constants(entry, errors);
	validate_social_publish_reservation_refs(entry.get("candidate_refs"), errors);
	validate_non_empty_string_list(entry.get("duplicate_keys"), "duplicate_keys", errors);
	validate_optional_string_list(entry.get("evidence_notes"), "evidence_notes", errors);
	validate_social_publish_reservation_owner(entry.get("owner"), errors);
	validate_rfc3339_field(entry, "reserved_at", errors);
	validate_rfc3339_field(entry, "expires_at", errors);
	validate_social_publish_reservation_status_payload(entry, errors);
}

fn validate_social_publish_reservation_constants(
	entry: &Map<String, Value>,
	errors: &mut Vec<String>,
) {
	if string_field(entry, "channel") != Some("x") {
		errors.push("channel must be x".into());
	}
	if string_field(entry, "target_account") != Some("decodexspace") {
		errors.push("target_account must be decodexspace".into());
	}
	if string_field(entry, "controller_account") != Some("hackink") {
		errors.push("controller_account must be hackink".into());
	}
	if !matches_one_of(entry.get("mode"), SOCIAL_POST_MODES) {
		errors.push(format!("mode must be one of {}", choices(SOCIAL_POST_MODES)));
	}
	if !matches_one_of(entry.get("status"), SOCIAL_PUBLISH_RESERVATION_STATUSES) {
		errors.push(format!(
			"status must be one of {}",
			choices(SOCIAL_PUBLISH_RESERVATION_STATUSES)
		));
	}
}

fn validate_social_publish_reservation_refs(refs: Option<&Value>, errors: &mut Vec<String>) {
	let Some(refs) = refs.and_then(Value::as_object) else {
		errors.push("candidate_refs must be an object".into());

		return;
	};
	let has_refs = ["social_candidates", "urls"]
		.iter()
		.any(|field| non_empty_array(refs.get(*field)).is_some());

	if !has_refs {
		errors.push("candidate_refs must include social_candidates or urls".into());
	}
	if refs.get("urls").is_some_and(|urls| !is_https_string_array(urls)) {
		errors.push("candidate_refs.urls must be a list of https URLs".into());
	}

	validate_optional_string_list(
		refs.get("social_candidates"),
		"candidate_refs.social_candidates",
		errors,
	);
}

fn validate_social_publish_reservation_owner(owner: Option<&Value>, errors: &mut Vec<String>) {
	let Some(owner) = owner else {
		return;
	};
	let Some(owner) = owner.as_object() else {
		errors.push("owner must be an object when present".into());

		return;
	};

	for field in ["automation_id", "branch", "pr_url", "run_id"] {
		if owner.get(field).is_some_and(|value| !is_non_empty_string(Some(value))) {
			errors.push(format!("owner.{field} must be non-empty when present"));
		}
	}

	if owner.get("pr_url").is_some_and(|value| !is_https_string(Some(value))) {
		errors.push("owner.pr_url must be an https URL when present".into());
	}
}

fn validate_social_publish_reservation_status_payload(
	entry: &Map<String, Value>,
	errors: &mut Vec<String>,
) {
	match string_field(entry, "status") {
		Some("consumed") if !is_non_empty_string(entry.get("consumed_by_social_post")) =>
			errors.push("consumed_by_social_post is required when status is consumed".into()),
		Some("canceled" | "expired") if !is_non_empty_string(entry.get("release_reason")) =>
			errors.push("release_reason is required when status is canceled or expired".into()),
		_ => {},
	}
}

fn validate_social_post(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	for field in ["slug", "audience"] {
		if !is_non_empty_string(entry.get(field)) {
			errors.push(format!("{field} must be a non-empty string"));
		}
	}

	validate_social_post_constants(entry, errors);
	validate_social_post_text(entry.get("text"), errors);
	validate_social_post_source_refs(entry.get("source_refs"), errors);

	for field in ["evidence_notes", "claims"] {
		if non_empty_array(entry.get(field)).is_none() {
			errors.push(format!("{field} must be a non-empty list"));
		}
	}

	validate_social_post_claims(entry.get("claims"), errors);
	validate_social_post_decision(entry, errors);
	validate_social_post_status_payload(entry, errors);
	validate_social_post_lifecycle(entry, errors);

	for field in ["caveats", "media_refs"] {
		validate_optional_string_list(entry.get(field), field, errors);
	}
}

fn validate_social_post_constants(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	if string_field(entry, "channel") != Some("x") {
		errors.push("channel must be x".into());
	}
	if string_field(entry, "target_account") != Some("decodexspace") {
		errors.push("target_account must be decodexspace".into());
	}
	if string_field(entry, "controller_account") != Some("hackink") {
		errors.push("controller_account must be hackink".into());
	}
	if !matches_one_of(entry.get("mode"), SOCIAL_POST_MODES) {
		errors.push(format!("mode must be one of {}", choices(SOCIAL_POST_MODES)));
	}
	if !matches_one_of(entry.get("status"), SOCIAL_POST_STATUSES) {
		errors.push(format!("status must be one of {}", choices(SOCIAL_POST_STATUSES)));
	}
}

fn validate_social_post_text(text: Option<&Value>, errors: &mut Vec<String>) {
	let Some(items) = non_empty_array(text) else {
		errors.push("text must be a non-empty list of X-sized strings".into());

		return;
	};

	for (index, text) in items.iter().enumerate() {
		let Some(text) = text.as_str() else {
			errors.push(format!("text[{index}] must be a string"));

			continue;
		};

		validate_social_post_text_item(text, index, errors);
	}
}

fn validate_social_post_text_item(text: &str, index: usize, errors: &mut Vec<String>) {
	if text.is_empty() || text.chars().count() > 280 {
		errors.push(format!("text[{index}] must be a non-empty X-sized string"));
	}
	if text.contains("Automated by @hackink") {
		errors.push(format!("text[{index}] must not include automation attribution"));
	}
	if text.chars().count() > 260 && !text.contains("https://") {
		errors.push(format!(
			"text[{index}] longer than 260 characters must include an unavoidable direct source URL"
		));
	}

	let normalized = text.trim().to_ascii_lowercase();

	if normalized == "watching this"
		|| normalized.starts_with("watching this.")
		|| normalized.starts_with("tracking this.")
		|| normalized.contains("new release available")
	{
		errors.push(format!(
			"text[{index}] must name a concrete source-backed release, PR, protocol surface, workflow impact, or operator action"
		));
	}
}

fn validate_social_post_source_refs(refs: Option<&Value>, errors: &mut Vec<String>) {
	let Some(refs) = refs.and_then(Value::as_object) else {
		errors.push("source_refs must be an object".into());

		return;
	};
	let has_refs = [
		"reservations",
		"signals",
		"social_candidates",
		"upstream_impacts",
		"upstream_reviews",
		"urls",
	]
	.iter()
	.any(|field| refs.get(*field).is_some_and(|value| !is_empty_or_missing_array(Some(value))));

	if !has_refs {
		errors.push(
			"source_refs must include reservations, signals, social_candidates, upstream_impacts, upstream_reviews, or urls"
				.into(),
		);
	}
	if refs.get("urls").is_some_and(|urls| !is_https_string_array(urls)) {
		errors.push("source_refs.urls must be a list of https URLs".into());
	}

	for field in
		["reservations", "signals", "social_candidates", "upstream_impacts", "upstream_reviews"]
	{
		validate_optional_string_list(refs.get(field), &format!("source_refs.{field}"), errors);
	}
}

fn validate_social_post_claims(claims: Option<&Value>, errors: &mut Vec<String>) {
	let Some(claims) = non_empty_array(claims) else {
		errors.push("claims must be a non-empty list of claim objects".into());

		return;
	};

	for (index, claim) in claims.iter().enumerate() {
		let Some(claim) = claim.as_object() else {
			errors.push(format!("claims[{index}] must be an object"));

			continue;
		};

		for field in ["text", "evidence"] {
			if !is_non_empty_string(claim.get(field)) {
				errors.push(format!("claims[{index}].{field} must be a non-empty string"));
			}
		}
		if !matches_one_of(claim.get("confidence"), SIGNAL_CONFIDENCE) {
			errors.push(format!(
				"claims[{index}].confidence must be one of {}",
				choices(SIGNAL_CONFIDENCE)
			));
		}
	}
}

fn validate_social_post_decision(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	let Some(decision) = entry.get("decision").and_then(Value::as_object) else {
		errors.push("decision must be an object".into());

		return;
	};

	if !matches_one_of(decision.get("worthiness"), SOCIAL_POST_WORTHINESS) {
		errors.push(format!(
			"decision.worthiness must be one of {}",
			choices(SOCIAL_POST_WORTHINESS)
		));
	}
	if !matches_one_of(decision.get("priority"), SOCIAL_POST_PRIORITIES) {
		errors
			.push(format!("decision.priority must be one of {}", choices(SOCIAL_POST_PRIORITIES)));
	}

	for field in ["idempotency_key", "reason", "day", "timezone"] {
		if !is_non_empty_string(decision.get(field)) {
			errors.push(format!("decision.{field} must be a non-empty string"));
		}
	}

	validate_social_post_decision_counts(entry, decision, errors);
}

fn validate_social_post_decision_counts(
	entry: &Map<String, Value>,
	decision: &Map<String, Value>,
	errors: &mut Vec<String>,
) {
	if decision.get("daily_limit").and_then(Value::as_i64) != Some(8) {
		errors.push("decision.daily_limit must be 8".into());
	}

	let before = decision.get("daily_count_before").and_then(Value::as_i64);
	let after = decision.get("daily_count_after").and_then(Value::as_i64);

	match string_field(entry, "status") {
		Some("published")
			if before.zip(after).is_none_or(|(before, after)| after != before + 1) =>
			errors.push(
				"decision.daily_count_after must equal daily_count_before + 1 for published posts"
					.into(),
			),
		Some("blocked" | "failed" | "skipped")
			if before.zip(after).is_none_or(|(before, after)| after != before) =>
			errors.push(
				"decision.daily_count_after must equal daily_count_before for non-published posts"
					.into(),
			),
		_ => {},
	}
}

fn validate_social_post_status_payload(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	match string_field(entry, "status") {
		Some("published") => validate_social_post_publication(entry.get("publication"), errors),
		Some("blocked") => validate_social_post_block(entry, errors),
		Some("failed") if !is_non_empty_string(entry.get("failure_reason")) =>
			errors.push("failure_reason is required when status is failed".into()),
		Some("skipped") if !is_non_empty_string(entry.get("skip_reason")) =>
			errors.push("skip_reason is required when status is skipped".into()),
		_ => {},
	}
}

fn validate_social_post_lifecycle(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	let Some(lifecycle) = entry.get("post_lifecycle") else {
		return;
	};
	let Some(lifecycle) = lifecycle.as_object() else {
		errors.push("post_lifecycle must be an object when present".into());

		return;
	};

	if !matches_one_of(lifecycle.get("current_state"), SOCIAL_POST_LIFECYCLE_STATES) {
		errors.push(format!(
			"post_lifecycle.current_state must be one of {}",
			choices(SOCIAL_POST_LIFECYCLE_STATES)
		));
	}
	if !lifecycle.get("quote_eligible").is_some_and(Value::is_boolean) {
		errors.push("post_lifecycle.quote_eligible must be a boolean".into());
	}
	if lifecycle.get("reason").is_some_and(|value| !is_non_empty_string(Some(value))) {
		errors.push("post_lifecycle.reason must be non-empty when present".into());
	}
	if lifecycle
		.get("superseded_by_candidate")
		.is_some_and(|value| !is_non_empty_string(Some(value)))
	{
		errors.push("post_lifecycle.superseded_by_candidate must be non-empty when present".into());
	}
	if lifecycle.get("current_state").and_then(Value::as_str) != Some("live")
		&& lifecycle.get("quote_eligible").and_then(Value::as_bool) == Some(true)
	{
		errors
			.push("post_lifecycle.quote_eligible can be true only for live published posts".into());
	}
}

fn validate_social_post_publication(publication: Option<&Value>, errors: &mut Vec<String>) {
	let Some(publication) = publication.and_then(Value::as_object) else {
		errors.push("publication must be an object when status is published".into());

		return;
	};

	for field in ["posted_at", "publisher"] {
		if !is_non_empty_string(publication.get(field)) {
			errors.push(format!("publication.{field} must be a non-empty string"));
		}
	}
	validate_rfc3339_field(publication, "posted_at", errors);
	if !publication.get("account_verified").is_some_and(Value::is_boolean) {
		errors.push("publication.account_verified must be a boolean".into());
	}
	if !publication.get("made_with_ai").is_some_and(Value::is_boolean) {
		errors.push("publication.made_with_ai must be a boolean".into());
	}
	if publication
		.get("published_urls")
		.is_some_and(|urls| !is_https_string_array(urls) || is_empty_or_missing_array(Some(urls)))
	{
		errors.push("publication.published_urls must be a non-empty list of https URLs".into());
	}
}

fn validate_social_post_block(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	if !matches_one_of(entry.get("block_reason"), SOCIAL_BLOCK_REASONS) {
		errors.push(format!("block_reason must be one of {}", choices(SOCIAL_BLOCK_REASONS)));
	}
}

fn validate_non_empty_string_list(value: Option<&Value>, label: &str, errors: &mut Vec<String>) {
	let valid = non_empty_array(value).is_some_and(|values| {
		values.iter().all(|item| item.as_str().is_some_and(|item| !item.is_empty()))
	});

	if !valid {
		errors.push(format!("{label} must be a non-empty list of strings"));
	}
}

fn validate_optional_string_list(value: Option<&Value>, label: &str, errors: &mut Vec<String>) {
	let Some(value) = value else {
		return;
	};

	if value.is_null() {
		return;
	}
	if !value.as_array().is_some_and(|values| {
		values.iter().all(|item| item.as_str().is_some_and(|item| !item.is_empty()))
	}) {
		errors.push(format!("{label} must be a list of non-empty strings when present"));
	}
}

fn validate_rfc3339_field(entry: &Map<String, Value>, field: &str, errors: &mut Vec<String>) {
	let Some(value) = entry.get(field).and_then(Value::as_str).filter(|value| !value.is_empty())
	else {
		return;
	};

	if OffsetDateTime::parse(value, &Rfc3339).is_err() {
		errors.push(format!("{field} must be an RFC3339 timestamp"));
	}
}

fn string_field<'a>(object: &'a Map<String, Value>, field: &str) -> Option<&'a str> {
	object.get(field).and_then(Value::as_str)
}

fn is_non_empty_string(value: Option<&Value>) -> bool {
	value.and_then(Value::as_str).is_some_and(|value| !value.is_empty())
}

fn matches_one_of(value: Option<&Value>, choices: &[&str]) -> bool {
	value.and_then(Value::as_str).is_some_and(|value| choices.contains(&value))
}

fn non_empty_array(value: Option<&Value>) -> Option<&Vec<Value>> {
	value.and_then(Value::as_array).filter(|values| !values.is_empty())
}

fn is_empty_or_missing_array(value: Option<&Value>) -> bool {
	value.and_then(Value::as_array).is_none_or(Vec::is_empty)
}

fn is_https_string(value: Option<&Value>) -> bool {
	value.and_then(Value::as_str).is_some_and(|value| value.starts_with("https://"))
}

fn is_https_string_array(value: &Value) -> bool {
	value.as_array().is_some_and(|values| values.iter().all(|url| is_https_string(Some(url))))
}

fn choices(values: &[&str]) -> String {
	let quoted = values.iter().map(|value| format!("'{value}'")).collect::<Vec<_>>().join(", ");

	format!("[{quoted}]")
}
