use std::{
	collections::{BTreeMap, BTreeSet},
	path::{Path, PathBuf},
};

use serde_json::{Map, Value};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::prelude::eyre::{self, Report};

use super::{
	ANALYSIS_DRAFT_KIND, BUNDLE_SCHEMA, CONFIG_FEATURE_CATALOG_SCHEMA,
	CONTROL_PLANE_UPGRADE_CANDIDATE_SCHEMA, RADAR_ARCHIVE_HISTORICAL_RETENTION_CUTOFF,
	RELEASE_DELTA_SCHEMA, SIGNAL_CONFIDENCE, SIGNAL_SCHEMA, SOCIAL_CANDIDATE_SCHEMA,
	SOCIAL_POST_SCHEMA, SOCIAL_PUBLISH_RESERVATION_SCHEMA, UPSTREAM_IMPACT_SCHEMA,
	UPSTREAM_REVIEW_LINEAR_FOLLOWUP_CUTOFF, UPSTREAM_REVIEW_QUEUE_SCHEMA, UPSTREAM_REVIEW_SCHEMA,
	UPSTREAM_SUBJECT_KINDS,
};

const RADAR_ARCHIVE_MANIFEST_SCHEMA: &str = "radar_archive_manifest/v1";
const ANALYSIS_MODES: &[&str] = &["commit_only", "pr_first"];
const SIGNAL_IMPACT: &[&str] = &["high", "low", "medium"];
const SIGNAL_KINDS: &[&str] = &["behavior_change", "capability", "try_now"];
const SOCIAL_BLOCK_REASONS: &[&str] =
	&["daily_cap_exceeded", "duplicate", "insufficient_evidence", "policy_block"];
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
const SOCIAL_POST_LIFECYCLE_STATES: &[&str] = &[
	"deleted_by_operator",
	"live",
	"superseded_failed_attempt",
	"superseded_published",
	"superseded_text_only",
];
const SOCIAL_PUBLISH_RESERVATION_STATUSES: &[&str] = &["active", "canceled", "consumed", "expired"];
const SOURCE_ITEM_KINDS: &[&str] = &["commit", "pull_request"];
const CONTROL_PLANE_UPGRADE_IMPACTS: &[&str] = &["adopt_now", "candidate", "compat_risk"];
const CONTROL_PLANE_UPGRADE_PATHS: &[&str] = &["adopt_now", "compat_risk_mitigation", "discovery"];
const CONTROL_PLANE_UPGRADE_STATUSES: &[&str] = &["blocked", "deferred", "proposed", "superseded"];
const CODEX_COMPATIBILITY_STATUSES: &[&str] =
	&["compatible", "incompatible", "needs_review", "not_tested", "unknown"];
const CODEX_TARGET_CHANNELS: &[&str] = &["main", "preview", "stable"];
const UPSTREAM_IMPACT_KINDS: &[&str] =
	&["browser_observation", "changelog", "commit", "pull_request", "release", "signal"];
const UPSTREAM_REVIEW_ACTION_TYPES: &[&str] = &[
	"control_plane_upgrade_candidate",
	"none",
	"signal_entry",
	"social_candidate",
	"upstream_impact",
];
const UPSTREAM_REVIEW_NEXT_STEPS: &[&str] = &["ai_review_required"];
const UPSTREAM_REVIEW_PRIORITIES: &[&str] = &["critical", "high", "low", "normal"];
const UPSTREAM_SOURCE_STATES: &[&str] = &["closed", "commit_only", "merged", "open"];

#[derive(Debug)]
pub(super) struct ArtifactValidation {
	pub(super) schema: Option<String>,
	pub(super) errors: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default)]
struct ArtifactValidationOptions {
	allow_historical_archive_retention: bool,
	allow_historical_upstream_review_linear_followup: bool,
}

#[derive(Debug)]
pub(super) struct ValidationState {
	active_social_publish_reservation_idempotency_keys: BTreeMap<String, PathBuf>,
	seen_terminal_social_post_idempotency_keys: BTreeMap<String, PathBuf>,
	seen_signal_slugs: BTreeMap<String, PathBuf>,
}
impl ValidationState {
	pub(super) fn new() -> Self {
		Self {
			active_social_publish_reservation_idempotency_keys: BTreeMap::new(),
			seen_terminal_social_post_idempotency_keys: BTreeMap::new(),
			seen_signal_slugs: BTreeMap::new(),
		}
	}
}

#[derive(Debug, Default)]
struct ReleaseOptionTags {
	stable: BTreeSet<String>,
	preview: BTreeSet<String>,
}

pub(super) fn validate_signal_file(path: &Path, payload: &Value) -> crate::prelude::Result<()> {
	let validation = validate_artifact(payload);

	if validation.schema.as_deref() != Some(SIGNAL_SCHEMA) || !validation.errors.is_empty() {
		eyre::bail!(
			"Signal validation failed for {}:\n- {}",
			path.display(),
			validation.errors.join("\n- ")
		);
	}

	Ok(())
}

pub(super) fn validate_analysis_draft(value: &Value) -> crate::prelude::Result<()> {
	let Some(draft) = value.as_object() else {
		return Err(eyre::eyre!("Analysis draft must be an object"));
	};
	let mut errors = Vec::new();

	for field in ["kind", "title", "summary", "why_it_matters", "confidence", "impact"] {
		if !is_non_empty_string(draft.get(field)) {
			errors.push(format!("{field} is required in analysis draft"));
		}
	}

	if !matches_one_of(draft.get("kind"), SIGNAL_KINDS) {
		errors.push(format!("kind must be one of {}", choices(SIGNAL_KINDS)));
	}
	if !matches_one_of(draft.get("confidence"), SIGNAL_CONFIDENCE) {
		errors.push(format!("confidence must be one of {}", choices(SIGNAL_CONFIDENCE)));
	}
	if !matches_one_of(draft.get("impact"), SIGNAL_IMPACT) {
		errors.push(format!("impact must be one of {}", choices(SIGNAL_IMPACT)));
	}
	if non_empty_array(draft.get("proof_points")).is_none() {
		errors.push("proof_points must be a non-empty list".into());
	}
	if string_field(draft, "kind") == Some("try_now")
		&& !is_truthy_json_value(draft.get("how_to_try"))
	{
		errors.push("how_to_try is required when kind is try_now".into());
	}
	if is_truthy_json_value(draft.get("how_to_try"))
		&& !is_truthy_json_value(draft.get("expected_effect"))
	{
		errors.push("expected_effect is required when how_to_try is present".into());
	}
	if errors.is_empty() {
		Ok(())
	} else {
		Err(eyre::eyre!("Analysis draft validation failed:\n- {}", errors.join("\n- ")))
	}
}

pub(super) fn validate_artifact_errors(payload: &Value) -> Vec<String> {
	validate_artifact(payload).errors
}

fn is_analysis_draft_path(path: &Path) -> bool {
	let normalized = normalized_path(path);

	normalized.ends_with(".analysis.json")
		&& (normalized.contains("/generated/analysis/")
			|| normalized.starts_with("generated/analysis/"))
}

fn is_historical_archive_manifest_path(path: &Path, payload: &Value) -> bool {
	let Some(entry) = payload.as_object() else {
		return false;
	};
	let normalized = normalized_path(path);

	string_field(entry, "schema") == Some(RADAR_ARCHIVE_MANIFEST_SCHEMA)
		&& normalized.contains("/cache/archive/index/")
		&& timestamp_field_before(entry, "created_at", RADAR_ARCHIVE_HISTORICAL_RETENTION_CUTOFF)
}

fn is_historical_upstream_review_path(path: &Path, payload: &Value) -> bool {
	let Some(entry) = payload.as_object() else {
		return false;
	};
	let normalized = normalized_path(path);

	string_field(entry, "schema") == Some(UPSTREAM_REVIEW_SCHEMA)
		&& normalized.contains("/cache/github/reviews/")
		&& timestamp_field_before(entry, "reviewed_at", UPSTREAM_REVIEW_LINEAR_FOLLOWUP_CUTOFF)
}

fn timestamp_field_before(entry: &Map<String, Value>, field: &str, cutoff: &str) -> bool {
	let Some(value) = entry.get(field).and_then(Value::as_str) else {
		return false;
	};
	let Ok(value) = OffsetDateTime::parse(value, &Rfc3339) else {
		return false;
	};
	let Ok(cutoff) = OffsetDateTime::parse(cutoff, &Rfc3339) else {
		return false;
	};

	value < cutoff
}

fn normalized_path(path: &Path) -> String {
	path.to_string_lossy().replace('\\', "/")
}

fn analysis_draft_error_lines(error: Report) -> Vec<String> {
	error
		.to_string()
		.lines()
		.map(str::trim)
		.filter(|line| !line.is_empty())
		.map(|line| line.trim_start_matches("- ").to_owned())
		.collect()
}

pub(super) fn validate_artifact(payload: &Value) -> ArtifactValidation {
	validate_artifact_with_options(payload, ArtifactValidationOptions::default())
}

pub(super) fn validate_artifact_for_path(path: &Path, payload: &Value) -> ArtifactValidation {
	if is_analysis_draft_path(path) && payload.get("schema").is_none() {
		return match validate_analysis_draft(payload) {
			Ok(()) => {
				ArtifactValidation { schema: Some(ANALYSIS_DRAFT_KIND.into()), errors: Vec::new() }
			},
			Err(error) => ArtifactValidation {
				schema: Some(ANALYSIS_DRAFT_KIND.into()),
				errors: analysis_draft_error_lines(error),
			},
		};
	}

	validate_artifact_with_options(
		payload,
		ArtifactValidationOptions {
			allow_historical_archive_retention: is_historical_archive_manifest_path(path, payload),
			allow_historical_upstream_review_linear_followup: is_historical_upstream_review_path(
				path, payload,
			),
		},
	)
}

fn validate_artifact_with_options(
	payload: &Value,
	options: ArtifactValidationOptions,
) -> ArtifactValidation {
	let Some(entry) = payload.as_object() else {
		return ArtifactValidation {
			schema: None,
			errors: vec!["artifact must be an object".into()],
		};
	};
	let schema = entry.get("schema").and_then(Value::as_str).map(str::to_owned);
	let mut errors = Vec::new();

	match schema.as_deref() {
		Some(BUNDLE_SCHEMA) => validate_bundle(entry, &mut errors),
		Some(CONFIG_FEATURE_CATALOG_SCHEMA) => validate_config_feature_catalog(entry, &mut errors),
		Some(CONTROL_PLANE_UPGRADE_CANDIDATE_SCHEMA) => {
			validate_control_plane_upgrade_candidate(entry, &mut errors)
		},
		Some(RADAR_ARCHIVE_MANIFEST_SCHEMA) => {
			validate_radar_archive_manifest(entry, options, &mut errors)
		},
		Some(RELEASE_DELTA_SCHEMA) => validate_release_delta(entry, &mut errors),
		Some(SIGNAL_SCHEMA) => validate_signal(entry, &mut errors),
		Some(SOCIAL_CANDIDATE_SCHEMA) => validate_social_candidate(entry, &mut errors),
		Some(SOCIAL_POST_SCHEMA) => validate_social_post(entry, &mut errors),
		Some(SOCIAL_PUBLISH_RESERVATION_SCHEMA) => {
			validate_social_publish_reservation(entry, &mut errors)
		},
		Some(UPSTREAM_IMPACT_SCHEMA) => validate_upstream_impact(entry, &mut errors),
		Some(UPSTREAM_REVIEW_QUEUE_SCHEMA) => validate_upstream_review_queue(entry, &mut errors),
		Some(UPSTREAM_REVIEW_SCHEMA) => validate_upstream_review(entry, options, &mut errors),
		Some(_) | None => errors.push(format!("schema must be one of {}", known_schemas())),
	}

	ArtifactValidation { schema, errors }
}

fn validate_bundle(bundle: &Map<String, Value>, errors: &mut Vec<String>) {
	if string_field(bundle, "repo").is_none_or(|repo| !repo.contains('/')) {
		errors.push("repo must be owner/name".into());
	}
	if !matches_one_of(bundle.get("analysis_mode"), ANALYSIS_MODES) {
		errors.push(format!("analysis_mode must be one of {}", choices(ANALYSIS_MODES)));
	}
	if !is_non_empty_string(bundle.get("default_branch")) {
		errors.push("default_branch must be a non-empty string".into());
	}

	validate_bundle_commits(bundle.get("commits"), errors);
	validate_bundle_files(bundle.get("files"), errors);

	if string_field(bundle, "analysis_mode") == Some("pr_first") {
		validate_bundle_pr(bundle.get("primary_pr"), errors);
	}
}

fn validate_bundle_commits(commits: Option<&Value>, errors: &mut Vec<String>) {
	let Some(commits) = non_empty_array(commits) else {
		errors.push("commits must be a non-empty list".into());

		return;
	};

	for (index, commit) in commits.iter().enumerate() {
		let Some(commit) = commit.as_object() else {
			errors.push(format!("commits[{index}] must be an object"));

			continue;
		};

		for field in ["sha", "message", "url"] {
			if !is_non_empty_string(commit.get(field)) {
				errors.push(format!("commits[{index}].{field} must be a non-empty string"));
			}
		}
	}
}

fn validate_bundle_files(files: Option<&Value>, errors: &mut Vec<String>) {
	let Some(files) = non_empty_array(files) else {
		errors.push("files must be a non-empty list".into());

		return;
	};

	for (index, item) in files.iter().enumerate() {
		let Some(item) = item.as_object() else {
			errors.push(format!("files[{index}] must be an object"));

			continue;
		};

		for field in ["path", "status", "additions", "deletions"] {
			if !item.contains_key(field) {
				errors.push(format!("files[{index}].{field} is required"));
			}
		}
	}
}

fn validate_bundle_pr(primary_pr: Option<&Value>, errors: &mut Vec<String>) {
	let Some(primary_pr) = primary_pr.and_then(Value::as_object) else {
		errors.push("primary_pr is required when analysis_mode is pr_first".into());

		return;
	};

	for field in ["number", "title", "body", "state", "labels", "url"] {
		if !primary_pr.contains_key(field) {
			errors.push(format!("primary_pr.{field} is required"));
		}
	}
}

fn validate_signal(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	if string_field(entry, "lane") != Some("github") {
		errors.push("lane must be github for the MVP".into());
	}
	if !matches_one_of(entry.get("kind"), SIGNAL_KINDS) {
		errors.push(format!("kind must be one of {}", choices(SIGNAL_KINDS)));
	}
	if !matches_one_of(entry.get("confidence"), SIGNAL_CONFIDENCE) {
		errors.push(format!("confidence must be one of {}", choices(SIGNAL_CONFIDENCE)));
	}
	if !matches_one_of(entry.get("impact"), SIGNAL_IMPACT) {
		errors.push(format!("impact must be one of {}", choices(SIGNAL_IMPACT)));
	}

	for field in ["slug", "title", "published_at", "summary", "why_it_matters"] {
		if !is_non_empty_string(entry.get(field)) {
			errors.push(format!("{field} must be a non-empty string"));
		}
	}

	validate_signal_lists(entry, errors);
	validate_signal_try_fields(entry, errors);
	validate_signal_source_refs(entry.get("source_refs"), errors);
	validate_multi_agent_v2_reference_text(entry, "signal entries", errors);
}

fn validate_config_feature_catalog(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	if !is_https_string(entry.get("source_url")) {
		errors.push("source_url must be an https URL".into());
	}
	if !is_non_empty_string(entry.get("generated_at")) {
		errors.push("generated_at must be a non-empty string".into());
	}

	let Some(features) = non_empty_array(entry.get("features")) else {
		errors.push("features must be a non-empty list".into());

		return;
	};

	if entry
		.get("feature_count")
		.and_then(Value::as_u64)
		.is_none_or(|count| count != features.len() as u64)
	{
		errors.push("feature_count must match features length".into());
	}

	let mut found_multi_agent_v2 = false;

	for (index, feature) in features.iter().enumerate() {
		let Some(feature) = feature.as_object() else {
			errors.push(format!("features[{index}] must be an object"));

			continue;
		};

		for field in [
			"name",
			"config_path",
			"toml_assignment",
			"toml_snippet",
			"cli_enable_flag",
			"schema_url",
			"reference_url",
			"github_search_url",
		] {
			if !is_non_empty_string(feature.get(field)) {
				errors.push(format!("features[{index}].{field} must be a non-empty string"));
			}
		}

		if string_field(feature, "name") == Some("multi_agent_v2") {
			found_multi_agent_v2 = true;

			validate_multi_agent_v2_catalog_feature(feature, index, errors);
		}
	}

	if !found_multi_agent_v2 {
		errors.push("features must include multi_agent_v2".into());
	}
}

fn validate_multi_agent_v2_catalog_feature(
	feature: &Map<String, Value>,
	index: usize,
	errors: &mut Vec<String>,
) {
	let Some(description) = feature.get("reference_description").and_then(Value::as_str) else {
		errors.push(format!(
			"features[{index}].reference_description must describe current followup_task behavior"
		));

		return;
	};
	let lower = description.to_ascii_lowercase();

	if !lower.contains("followup_task") {
		errors.push(format!(
			"features[{index}].reference_description must mention current followup_task behavior"
		));
	}
	if lower.contains("assign_task") && !has_legacy_multi_agent_v2_context(&lower) {
		errors.push(format!(
			"features[{index}].reference_description must label assign_task as legacy or renamed context"
		));
	}
}

fn validate_multi_agent_v2_reference_text(
	entry: &Map<String, Value>,
	label: &str,
	errors: &mut Vec<String>,
) {
	let mut text = String::new();

	collect_json_strings_from_map(entry, &mut text);

	let lower = text.to_ascii_lowercase();
	let mentions_v2 = lower.contains("multiagentv2")
		|| lower.contains("multi_agent_v2")
		|| lower.contains("multi-agent v2");

	if !mentions_v2 || !lower.contains("assign_task") {
		return;
	}
	if !lower.contains("followup_task") {
		errors.push(format!(
			"{label} that mention MultiAgentV2 assign_task must also mention current followup_task"
		));
	}
	if !has_legacy_multi_agent_v2_context(&lower) {
		errors.push(format!(
			"{label} must describe assign_task as legacy, historical, older, previous, or renamed context"
		));
	}
}

pub(super) fn has_legacy_multi_agent_v2_context(text: &str) -> bool {
	["legacy", "historical", "older", "previous", "renamed", "rename"]
		.into_iter()
		.any(|term| text.contains(term))
}

fn collect_json_strings_from_map(object: &Map<String, Value>, text: &mut String) {
	for value in object.values() {
		collect_json_strings(value, text);
	}
}

fn collect_json_strings(value: &Value, text: &mut String) {
	match value {
		Value::String(value) => {
			text.push(' ');
			text.push_str(value);
		},
		Value::Array(values) => {
			for value in values {
				collect_json_strings(value, text);
			}
		},
		Value::Object(object) => collect_json_strings_from_map(object, text),
		Value::Bool(_) | Value::Null | Value::Number(_) => {},
	}
}

fn validate_signal_lists(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	if non_empty_array(entry.get("proof_points")).is_none() {
		errors.push("proof_points must be a non-empty list".into());
	}
}

fn validate_signal_try_fields(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	let config_flags_present = optional_array(entry.get("config_flags"), "config_flags", errors)
		.is_some_and(|values| !values.is_empty());
	let how_to_try = entry.get("how_to_try");

	if (string_field(entry, "kind") == Some("try_now") || config_flags_present)
		&& !is_truthy_json_value(how_to_try)
	{
		errors.push("how_to_try is required for try_now or flag-backed entries".into());
	}
	if is_truthy_json_value(how_to_try) && !is_truthy_json_value(entry.get("expected_effect")) {
		errors.push("expected_effect is required when how_to_try is present".into());
	}

	validate_optional_string_list(entry.get("caveats"), "caveats", errors);

	let watch_state = entry.get("watch_state");

	if watch_state.is_some() && !watch_state.is_some_and(|value| is_non_empty_string(Some(value))) {
		errors.push("watch_state must be a non-empty string when present".into());
	}
}

fn validate_signal_source_refs(refs: Option<&Value>, errors: &mut Vec<String>) {
	let Some(refs) = refs.and_then(Value::as_object) else {
		errors.push("source_refs must be an object".into());

		return;
	};

	if string_field(refs, "repo").is_none_or(|repo| !repo.contains('/')) {
		errors.push("source_refs.repo must be owner/name".into());
	}

	validate_signal_source_items(refs.get("items"), errors);

	let pr_url = refs.get("pr_url");
	let commit_urls = refs.get("commit_urls");
	let items = refs.get("items");

	if pr_url.is_none()
		&& is_empty_or_missing_array(commit_urls)
		&& is_empty_or_missing_array(items)
	{
		errors.push("source_refs must include pr_url, commit URLs, or source_refs.items".into());
	}
	if pr_url.is_some_and(|url| !is_https_string(Some(url))) {
		errors.push("source_refs.pr_url must be an https URL when present".into());
	}
	if commit_urls.is_some_and(|urls| !is_https_string_array(urls)) {
		errors.push("source_refs.commit_urls must be a list of https URLs".into());
	}
}

fn validate_signal_source_items(items: Option<&Value>, errors: &mut Vec<String>) {
	let Some(items) = items else {
		return;
	};

	if items.as_array().is_some_and(Vec::is_empty) {
		return;
	}

	let valid = items.as_array().is_some_and(|items| {
		items.iter().all(|item| {
			item.as_object().is_some_and(|item| {
				matches_one_of(item.get("kind"), SOURCE_ITEM_KINDS)
					&& is_non_empty_string(item.get("title"))
					&& is_https_string(item.get("url"))
					&& item.get("meta").is_none_or(|meta| meta.as_str().is_some())
			})
		})
	});

	if !valid {
		errors.push("source_refs.items must be a list of titled source entries".into());
	}
}

fn validate_release_delta(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	if string_field(entry, "repo").is_none_or(|repo| !repo.contains('/')) {
		errors.push("repo must be owner/name".into());
	}
	if !is_non_empty_string(entry.get("tag_prefix")) {
		errors.push("tag_prefix must be a non-empty string".into());
	}
	if !is_non_empty_string(entry.get("generated_at")) {
		errors.push("generated_at must be a non-empty string".into());
	}

	let tag_prefix = string_field(entry, "tag_prefix").unwrap_or_default();

	validate_release_object(
		entry.get("stable_release"),
		"stable_release",
		tag_prefix,
		false,
		errors,
	);
	validate_release_object(entry.get("prerelease"), "prerelease", tag_prefix, true, errors);
	validate_compare_object(entry.get("compare"), "compare", errors);
	validate_string_list(entry.get("tracked_signal_slugs"), "tracked_signal_slugs", errors);

	let option_tags = validate_release_options(entry.get("release_options"), errors);

	validate_release_comparisons(entry, &option_tags, errors);
}

fn validate_release_object(
	release: Option<&Value>,
	field_name: &str,
	tag_prefix: &str,
	expect_prerelease: bool,
	errors: &mut Vec<String>,
) {
	let Some(release) = release.and_then(Value::as_object) else {
		errors.push(format!("{field_name} must be an object"));

		return;
	};

	for field in ["tag_name", "name", "published_at", "url"] {
		if !is_non_empty_string(release.get(field)) {
			errors.push(format!("{field_name}.{field} must be a non-empty string"));
		}
	}

	if string_field(release, "tag_name").is_some_and(|tag_name| !tag_name.starts_with(tag_prefix)) {
		errors.push(format!("{field_name}.tag_name must start with tag_prefix"));
	}
	if release.get("prerelease").and_then(Value::as_bool) != Some(expect_prerelease) {
		let expected = if expect_prerelease { "true" } else { "false" };

		errors.push(format!("{field_name}.prerelease must be {expected}"));
	}
}

fn validate_compare_object(compare: Option<&Value>, label: &str, errors: &mut Vec<String>) {
	let Some(compare) = compare.and_then(Value::as_object) else {
		errors.push(format!("{label} must be an object"));

		return;
	};

	if !is_non_empty_string(compare.get("status")) {
		errors.push(format!("{label}.status must be a non-empty string"));
	}

	for field in ["ahead_by", "total_commits"] {
		if compare.get(field).and_then(Value::as_i64).is_none() {
			errors.push(format!("{label}.{field} must be an integer"));
		}
	}

	if !is_https_string(compare.get("url")) {
		errors.push(format!("{label}.url must be an https URL"));
	}

	validate_optional_string_list(
		compare.get("commit_shas"),
		&format!("{label}.commit_shas"),
		errors,
	);
	validate_optional_positive_integer_list(
		compare.get("pr_numbers"),
		&format!("{label}.pr_numbers"),
		errors,
	);
}

fn validate_release_options(
	options: Option<&Value>,
	errors: &mut Vec<String>,
) -> ReleaseOptionTags {
	let mut tags = ReleaseOptionTags::default();
	let Some(options) = options.and_then(Value::as_object) else {
		errors.push("release_options must be an object".into());

		return tags;
	};

	validate_release_option_group(
		options.get("stable"),
		"release_options.stable",
		false,
		errors,
		&mut tags.stable,
	);
	validate_release_option_group(
		options.get("preview"),
		"release_options.preview",
		true,
		errors,
		&mut tags.preview,
	);

	tags
}

fn validate_release_option_group(
	values: Option<&Value>,
	label: &str,
	expect_prerelease: bool,
	errors: &mut Vec<String>,
	tags: &mut BTreeSet<String>,
) {
	let Some(values) = non_empty_array(values) else {
		errors.push(format!("{label} must be a non-empty list"));

		return;
	};

	for (index, release) in values.iter().enumerate() {
		let Some(release) = release.as_object() else {
			errors.push(format!("{label}[{index}] must be an object"));

			continue;
		};

		if let Some(tag_name) = string_field(release, "tag_name") {
			if tag_name.is_empty() {
				errors.push(format!("{label}[{index}].tag_name must be a non-empty string"));
			} else {
				tags.insert(tag_name.to_owned());
			}
		} else {
			errors.push(format!("{label}[{index}].tag_name must be a non-empty string"));
		}

		if release.get("prerelease").and_then(Value::as_bool) != Some(expect_prerelease) {
			let expected = if expect_prerelease { "true" } else { "false" };

			errors.push(format!("{label}[{index}].prerelease must be {expected}"));
		}
	}
}

fn validate_release_comparisons(
	entry: &Map<String, Value>,
	option_tags: &ReleaseOptionTags,
	errors: &mut Vec<String>,
) {
	let Some(comparisons) = non_empty_array(entry.get("comparisons")) else {
		errors.push("comparisons must be a non-empty list".into());

		return;
	};
	let stable_release = entry.get("stable_release").and_then(Value::as_object);
	let prerelease = entry.get("prerelease").and_then(Value::as_object);
	let mut has_default_comparison = false;

	for (index, comparison) in comparisons.iter().enumerate() {
		let Some(comparison) = comparison.as_object() else {
			errors.push(format!("comparisons[{index}] must be an object"));

			continue;
		};

		validate_release_comparison_tags(comparison, index, option_tags, errors);

		if comparison_matches_default(comparison, stable_release, prerelease) {
			has_default_comparison = true;
		}

		validate_compare_object(
			comparison.get("compare"),
			&format!("comparisons[{index}].compare"),
			errors,
		);
		validate_string_list(
			comparison.get("tracked_signal_slugs"),
			&format!("comparisons[{index}].tracked_signal_slugs"),
			errors,
		);
	}

	if !has_default_comparison {
		errors.push("comparisons must include the default stable/prerelease pair".into());
	}
}

fn validate_release_comparison_tags(
	comparison: &Map<String, Value>,
	index: usize,
	option_tags: &ReleaseOptionTags,
	errors: &mut Vec<String>,
) {
	match string_field(comparison, "stable_tag_name") {
		Some("") => {
			errors.push(format!("comparisons[{index}].stable_tag_name must be a non-empty string"))
		},
		Some(tag_name)
			if !option_tags.stable.is_empty() && !option_tags.stable.contains(tag_name) =>
		{
			errors.push(format!(
				"comparisons[{index}].stable_tag_name must exist in release_options.stable"
			))
		},
		Some(_) => {},
		None => {
			errors.push(format!("comparisons[{index}].stable_tag_name must be a non-empty string"))
		},
	}
	match string_field(comparison, "prerelease_tag_name") {
		Some("") => errors
			.push(format!("comparisons[{index}].prerelease_tag_name must be a non-empty string")),
		Some(tag_name)
			if !option_tags.preview.is_empty() && !option_tags.preview.contains(tag_name) =>
		{
			errors.push(format!(
				"comparisons[{index}].prerelease_tag_name must exist in release_options.preview"
			))
		},
		Some(_) => {},
		None => errors
			.push(format!("comparisons[{index}].prerelease_tag_name must be a non-empty string")),
	}
}

fn comparison_matches_default(
	comparison: &Map<String, Value>,
	stable_release: Option<&Map<String, Value>>,
	prerelease: Option<&Map<String, Value>>,
) -> bool {
	let stable_tag = stable_release.and_then(|release| string_field(release, "tag_name"));
	let prerelease_tag = prerelease.and_then(|release| string_field(release, "tag_name"));

	string_field(comparison, "stable_tag_name") == stable_tag
		&& string_field(comparison, "prerelease_tag_name") == prerelease_tag
}

fn validate_upstream_review_queue(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	if string_field(entry, "repo").is_none_or(|repo| !repo.contains('/')) {
		errors.push("repo must be owner/name".into());
	}
	if !is_non_empty_string(entry.get("generated_at")) {
		errors.push("generated_at must be a non-empty string".into());
	}

	validate_upstream_review_queue_source(entry.get("source"), errors);

	let subjects = validate_upstream_review_subjects(entry.get("subjects"), errors);

	validate_upstream_review_counts(entry.get("counts"), subjects, errors);
}

fn validate_upstream_review_queue_source(source: Option<&Value>, errors: &mut Vec<String>) {
	let Some(source) = source.and_then(Value::as_object) else {
		errors.push("source must be an object".into());

		return;
	};

	if !is_non_empty_string(source.get("default_branch")) {
		errors.push("source.default_branch must be a non-empty string".into());
	}
	if source.get("search_limit").and_then(Value::as_i64).is_none_or(|value| value < 1) {
		errors.push("source.search_limit must be a positive integer".into());
	}
}

fn validate_upstream_review_subjects(subjects: Option<&Value>, errors: &mut Vec<String>) -> usize {
	let Some(subjects) = subjects.and_then(Value::as_array) else {
		errors.push("subjects must be a list".into());

		return 0;
	};
	let mut seen = BTreeSet::new();

	for (index, subject) in subjects.iter().enumerate() {
		let Some(subject) = subject.as_object() else {
			errors.push(format!("subjects[{index}] must be an object"));

			continue;
		};

		validate_upstream_review_subject(subject, index, &mut seen, errors);
	}

	subjects.len()
}

fn validate_upstream_review_subject(
	subject: &Map<String, Value>,
	index: usize,
	seen: &mut BTreeSet<(String, String)>,
	errors: &mut Vec<String>,
) {
	let subject_kind = string_field(subject, "subject_kind");
	let subject_id = string_field(subject, "subject_id");

	if !matches_one_of(subject.get("subject_kind"), UPSTREAM_SUBJECT_KINDS) {
		errors.push(format!(
			"subjects[{index}].subject_kind must be one of {}",
			choices(UPSTREAM_SUBJECT_KINDS)
		));
	}
	if !is_non_empty_string(subject.get("subject_id")) {
		errors.push(format!("subjects[{index}].subject_id must be a non-empty string"));
	}

	if let (Some(subject_kind), Some(subject_id)) = (subject_kind, subject_id) {
		let key = (subject_kind.to_owned(), subject_id.to_owned());

		if !seen.insert(key) {
			errors.push(format!("subjects[{index}] duplicates {subject_kind}:{subject_id}"));
		}
	}

	validate_upstream_review_subject_fields(subject, index, errors);
}

fn validate_upstream_review_subject_fields(
	subject: &Map<String, Value>,
	index: usize,
	errors: &mut Vec<String>,
) {
	for field in ["title", "url", "review_reason"] {
		if !is_non_empty_string(subject.get(field)) {
			errors.push(format!("subjects[{index}].{field} must be a non-empty string"));
		}
	}

	if !is_https_string(subject.get("url")) {
		errors.push(format!("subjects[{index}].url must be an https URL"));
	}
	if !matches_one_of(subject.get("source_state"), UPSTREAM_SOURCE_STATES) {
		errors.push(format!(
			"subjects[{index}].source_state must be one of {}",
			choices(UPSTREAM_SOURCE_STATES)
		));
	}
	if !matches_one_of(subject.get("review_priority"), UPSTREAM_REVIEW_PRIORITIES) {
		errors.push(format!(
			"subjects[{index}].review_priority must be one of {}",
			choices(UPSTREAM_REVIEW_PRIORITIES)
		));
	}
	if !matches_one_of(subject.get("next_step"), UPSTREAM_REVIEW_NEXT_STEPS) {
		errors.push(format!(
			"subjects[{index}].next_step must be one of {}",
			choices(UPSTREAM_REVIEW_NEXT_STEPS)
		));
	}

	validate_non_empty_string_list(
		subject.get("commit_shas"),
		&format!("subjects[{index}].commit_shas"),
		errors,
	);

	for field in ["surface_hints", "attention_flags", "sample_paths"] {
		validate_optional_string_list(
			subject.get(field),
			&format!("subjects[{index}].{field}"),
			errors,
		);
	}

	if subject.get("changed_file_count").and_then(Value::as_i64).is_none_or(|value| value < 0) {
		errors.push(format!("subjects[{index}].changed_file_count must be a non-negative integer"));
	}
}

fn validate_upstream_review_counts(
	counts: Option<&Value>,
	subjects: usize,
	errors: &mut Vec<String>,
) {
	let Some(counts) = counts.and_then(Value::as_object) else {
		errors.push("counts must be an object".into());

		return;
	};

	if counts.get("subjects_queued").and_then(Value::as_u64) != Some(subjects as u64) {
		errors.push("counts.subjects_queued must equal len(subjects)".into());
	}

	for field in
		["recent_commits_scanned", "published_subjects_seen", "critical", "high", "normal", "low"]
	{
		if counts.get(field).and_then(Value::as_i64).is_none_or(|value| value < 0) {
			errors.push(format!("counts.{field} must be a non-negative integer"));
		}
	}
}

fn validate_upstream_review(
	entry: &Map<String, Value>,
	options: ArtifactValidationOptions,
	errors: &mut Vec<String>,
) {
	for field in ["slug", "repo", "reviewed_at", "observed_change"] {
		if !is_non_empty_string(entry.get(field)) {
			errors.push(format!("{field} must be a non-empty string"));
		}
	}

	if string_field(entry, "repo").is_some_and(|repo| !repo.contains('/')) {
		errors.push("repo must be owner/name".into());
	}

	validate_upstream_review_subject_object(entry.get("subject"), errors);
	validate_upstream_review_source_refs(entry.get("source_refs"), errors);

	for field in ["changed_surfaces", "evidence"] {
		validate_non_empty_string_list(entry.get(field), field, errors);
	}

	validate_upstream_review_optional_strings(entry, errors);

	if !matches_one_of(entry.get("confidence"), SIGNAL_CONFIDENCE) {
		errors.push(format!("confidence must be one of {}", choices(SIGNAL_CONFIDENCE)));
	}

	validate_upstream_review_actions(entry.get("next_actions"), options, errors);
}

fn validate_upstream_review_subject_object(subject: Option<&Value>, errors: &mut Vec<String>) {
	let Some(subject) = subject.and_then(Value::as_object) else {
		errors.push("subject must be an object".into());

		return;
	};

	if !matches_one_of(subject.get("subject_kind"), UPSTREAM_SUBJECT_KINDS) {
		errors.push(format!(
			"subject.subject_kind must be one of {}",
			choices(UPSTREAM_SUBJECT_KINDS)
		));
	}
	if !is_non_empty_string(subject.get("subject_id")) {
		errors.push("subject.subject_id must be a non-empty string".into());
	}

	validate_optional_string_list(subject.get("commit_shas"), "subject.commit_shas", errors);
}

fn validate_upstream_review_source_refs(refs: Option<&Value>, errors: &mut Vec<String>) {
	let Some(refs) = refs.and_then(Value::as_object) else {
		errors.push("source_refs must be an object".into());

		return;
	};
	let valid = non_empty_array(refs.get("items")).is_some_and(|items| {
		items.iter().all(|item| {
			item.as_object().is_some_and(|item| {
				is_non_empty_string(item.get("kind"))
					&& is_non_empty_string(item.get("title"))
					&& is_https_string(item.get("url"))
			})
		})
	});

	if !valid {
		errors.push(
			"source_refs.items must be a non-empty list of titled https source entries".into(),
		);
	}
}

fn validate_upstream_review_optional_strings(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	for field in [
		"user_visible_path",
		"control_plane_relevance",
		"compatibility_risk",
		"adoption_opportunity",
		"community_value",
		"deprecated_or_breaking_notes",
		"caveats",
	] {
		if entry.get(field).is_some_and(|value| !value.is_string() && !value.is_null()) {
			errors.push(format!("{field} must be a string when present"));
		}
	}
}

fn validate_upstream_review_actions(
	next_actions: Option<&Value>,
	options: ArtifactValidationOptions,
	errors: &mut Vec<String>,
) {
	let Some(next_actions) = non_empty_array(next_actions) else {
		errors.push("next_actions must be a non-empty list".into());

		return;
	};

	for (index, action) in next_actions.iter().enumerate() {
		let Some(action) = action.as_object() else {
			errors.push(format!("next_actions[{index}] must be an object"));

			continue;
		};

		let legacy_linear_followup = options.allow_historical_upstream_review_linear_followup
			&& string_field(action, "type") == Some("linear_followup");

		if !legacy_linear_followup
			&& !matches_one_of(action.get("type"), UPSTREAM_REVIEW_ACTION_TYPES)
		{
			errors.push(format!(
				"next_actions[{index}].type must be one of {}",
				choices(UPSTREAM_REVIEW_ACTION_TYPES)
			));
		}
		if !is_non_empty_string(action.get("reason")) {
			errors.push(format!("next_actions[{index}].reason must be a non-empty string"));
		}
	}
}

fn validate_upstream_impact(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	for field in ["slug", "repo", "observed_change"] {
		if !is_non_empty_string(entry.get(field)) {
			errors.push(format!("{field} must be a non-empty string"));
		}
	}

	if string_field(entry, "repo").is_some_and(|repo| !repo.contains('/')) {
		errors.push("repo must be owner/name".into());
	}

	validate_upstream_impact_source_refs(entry.get("source_refs"), errors);

	if !matches_one_of(entry.get("public_signal_decision"), &["defer", "publish", "skip"]) {
		errors.push("public_signal_decision must be one of ['defer', 'publish', 'skip']".into());
	}
	if !matches_one_of(
		entry.get("control_plane_impact"),
		&["adopt_now", "candidate", "compat_risk", "none", "watch"],
	) {
		errors.push("control_plane_impact must be one of ['adopt_now', 'candidate', 'compat_risk', 'none', 'watch']".into());
	}
	if !matches_one_of(
		entry.get("publisher_angle"),
		&["none", "operator_impact", "practical_explainer", "release_pulse", "watch_note"],
	) {
		errors.push("publisher_angle must be one of ['none', 'operator_impact', 'practical_explainer', 'release_pulse', 'watch_note']".into());
	}
	if !matches_one_of(entry.get("confidence"), SIGNAL_CONFIDENCE) {
		errors.push(format!("confidence must be one of {}", choices(SIGNAL_CONFIDENCE)));
	}

	validate_non_empty_string_list(entry.get("evidence"), "evidence", errors);

	for field in ["candidate_followups", "social_notes", "caveats"] {
		validate_optional_string_list(entry.get(field), field, errors);
	}
}

fn validate_control_plane_upgrade_candidate(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	for field in ["slug", "repo", "observed_change", "reason"] {
		if !is_non_empty_string(entry.get(field)) {
			errors.push(format!("{field} must be a non-empty string"));
		}
	}

	if string_field(entry, "repo").is_some_and(|repo| !repo.contains('/')) {
		errors.push("repo must be owner/name".into());
	}
	if !matches_one_of(entry.get("status"), CONTROL_PLANE_UPGRADE_STATUSES) {
		errors.push(format!("status must be one of {}", choices(CONTROL_PLANE_UPGRADE_STATUSES)));
	}
	if !matches_one_of(entry.get("control_plane_impact"), CONTROL_PLANE_UPGRADE_IMPACTS) {
		errors.push(format!(
			"control_plane_impact must be one of {}",
			choices(CONTROL_PLANE_UPGRADE_IMPACTS)
		));
	}
	if !matches_one_of(entry.get("upgrade_path"), CONTROL_PLANE_UPGRADE_PATHS) {
		errors
			.push(format!("upgrade_path must be one of {}", choices(CONTROL_PLANE_UPGRADE_PATHS)));
	}

	validate_control_plane_upgrade_source_refs(entry.get("source_refs"), errors);
	validate_control_plane_upgrade_target_codex(entry.get("target_codex"), errors);
	validate_control_plane_upgrade_authority(entry.get("authority"), errors);
	validate_non_empty_string_list(entry.get("affected_surfaces"), "affected_surfaces", errors);
	validate_non_empty_string_list(entry.get("validation_gates"), "validation_gates", errors);
	validate_non_empty_string_list(entry.get("stop_conditions"), "stop_conditions", errors);

	for field in ["acceptance_criteria", "caveats", "next_steps"] {
		validate_optional_string_list(entry.get(field), field, errors);
	}
}

fn validate_control_plane_upgrade_source_refs(refs: Option<&Value>, errors: &mut Vec<String>) {
	let Some(refs) = refs.and_then(Value::as_object) else {
		errors.push("source_refs must be an object".into());

		return;
	};
	let has_refs = ["upstream_reviews", "upstream_impacts", "release_deltas", "urls"]
		.iter()
		.any(|field| non_empty_array(refs.get(*field)).is_some());

	if !has_refs {
		errors.push(
			"source_refs must include upstream_reviews, upstream_impacts, release_deltas, or urls"
				.into(),
		);
	}
	if non_empty_array(refs.get("upstream_impacts")).is_none() {
		errors.push(
			"source_refs.upstream_impacts must include the shared upstream_impact/v1 handoff"
				.into(),
		);
	}
	if refs.get("urls").is_some_and(|urls| !is_https_string_array(urls)) {
		errors.push("source_refs.urls must be a list of https URLs".into());
	}

	for field in ["upstream_reviews", "upstream_impacts", "release_deltas"] {
		validate_optional_string_list(refs.get(field), &format!("source_refs.{field}"), errors);
	}
}

fn validate_control_plane_upgrade_target_codex(target: Option<&Value>, errors: &mut Vec<String>) {
	let Some(target) = target.and_then(Value::as_object) else {
		errors.push("target_codex must be an object".into());

		return;
	};

	if !matches_one_of(target.get("channel"), CODEX_TARGET_CHANNELS) {
		errors.push(format!(
			"target_codex.channel must be one of {}",
			choices(CODEX_TARGET_CHANNELS)
		));
	}
	if !["version", "tag", "commit_sha", "release_url"]
		.iter()
		.any(|field| is_non_empty_string(target.get(*field)))
	{
		errors.push("target_codex must include version, tag, commit_sha, or release_url".into());
	}
	if target.get("release_url").is_some_and(|url| !is_https_string(Some(url))) {
		errors.push("target_codex.release_url must be an https URL when present".into());
	}
	if target
		.get("compatibility_status")
		.is_some_and(|status| !matches_one_of(Some(status), CODEX_COMPATIBILITY_STATUSES))
	{
		errors.push(format!(
			"target_codex.compatibility_status must be one of {}",
			choices(CODEX_COMPATIBILITY_STATUSES)
		));
	}

	for field in ["version", "tag", "commit_sha", "matrix_ref", "probe_evidence"] {
		if target.get(field).is_some_and(|value| !is_non_empty_string(Some(value))) {
			errors.push(format!("target_codex.{field} must be non-empty when present"));
		}
	}
}

fn validate_control_plane_upgrade_authority(authority: Option<&Value>, errors: &mut Vec<String>) {
	let Some(authority) = authority.and_then(Value::as_object) else {
		errors.push("authority must be an object".into());

		return;
	};

	for field in ["decision_contract_required", "program_intake_required"] {
		if authority.get(field).and_then(Value::as_bool) != Some(true) {
			errors.push(format!("authority.{field} must be true"));
		}
	}

	if authority.get("mutation_allowed").and_then(Value::as_bool) != Some(false) {
		errors.push("authority.mutation_allowed must be false".into());
	}

	for field in ["objective_id", "objective_version", "policy_ref"] {
		if authority.get(field).is_some_and(|value| !is_non_empty_string(Some(value))) {
			errors.push(format!("authority.{field} must be non-empty when present"));
		}
	}
}

fn validate_upstream_impact_source_refs(refs: Option<&Value>, errors: &mut Vec<String>) {
	let Some(refs) = refs.and_then(Value::as_object) else {
		errors.push("source_refs must be an object".into());

		return;
	};
	let valid = non_empty_array(refs.get("items")).is_some_and(|items| {
		items.iter().all(|item| {
			item.as_object().is_some_and(|item| {
				matches_one_of(item.get("kind"), UPSTREAM_IMPACT_KINDS)
					&& is_non_empty_string(item.get("title"))
					&& is_https_string(item.get("url"))
					&& item.get("meta").is_none_or(|meta| is_non_empty_string(Some(meta)))
			})
		})
	});

	if !valid {
		errors.push(
			"source_refs.items must be a non-empty list of titled https source entries".into(),
		);
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

fn validate_radar_archive_manifest(
	entry: &Map<String, Value>,
	options: ArtifactValidationOptions,
	errors: &mut Vec<String>,
) {
	for field in ["archive_id", "created_at", "source_commit", "release_tag", "release_url"] {
		if !is_non_empty_string(entry.get(field)) {
			errors.push(format!("{field} must be a non-empty string"));
		}
	}

	validate_rfc3339_field(entry, "created_at", errors);

	if entry.get("retention_days").and_then(Value::as_u64) != Some(21)
		&& !options.allow_historical_archive_retention
	{
		errors.push("retention_days must be 21".into());
	}
	if !is_https_string(entry.get("release_url")) {
		errors.push("release_url must be an https URL".into());
	}

	validate_archive_asset(entry.get("archive_asset"), "archive_asset", true, errors);
	validate_archive_asset(entry.get("checksum_asset"), "checksum_asset", false, errors);
	validate_archive_files(entry.get("files"), errors);
}

fn validate_archive_asset(
	value: Option<&Value>,
	label: &str,
	require_size: bool,
	errors: &mut Vec<String>,
) {
	let Some(asset) = value.and_then(Value::as_object) else {
		errors.push(format!("{label} must be an object"));

		return;
	};

	if !is_non_empty_string(asset.get("name")) {
		errors.push(format!("{label}.name must be a non-empty string"));
	}
	if !asset.get("sha256").and_then(Value::as_str).is_some_and(is_sha256_hex) {
		errors.push(format!("{label}.sha256 must be a SHA-256 hex digest"));
	}
	if require_size && asset.get("size_bytes").and_then(Value::as_u64).is_none_or(|size| size == 0)
	{
		errors.push(format!("{label}.size_bytes must be a positive integer"));
	}
}

fn validate_archive_files(value: Option<&Value>, errors: &mut Vec<String>) {
	let Some(files) = non_empty_array(value) else {
		errors.push("files must be a non-empty list".into());

		return;
	};

	for (index, file) in files.iter().enumerate() {
		let Some(file) = file.as_object() else {
			errors.push(format!("files[{index}] must be an object"));

			continue;
		};

		for field in ["path", "kind"] {
			if !is_non_empty_string(file.get(field)) {
				errors.push(format!("files[{index}].{field} must be a non-empty string"));
			}
		}

		if !matches_one_of(
			file.get("kind"),
			&["analysis", "bundle", "ledger_export", "other", "source_cache"],
		) {
			errors.push(format!(
				"files[{index}].kind must be one of ['analysis', 'bundle', 'ledger_export', 'other', 'source_cache']"
			));
		}
		if !file.get("sha256").and_then(Value::as_str).is_some_and(is_sha256_hex) {
			errors.push(format!("files[{index}].sha256 must be a SHA-256 hex digest"));
		}
		if file.get("size_bytes").and_then(Value::as_u64).is_none_or(|size| size == 0) {
			errors.push(format!("files[{index}].size_bytes must be a positive integer"));
		}
	}
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
		Some("consumed") if !is_non_empty_string(entry.get("consumed_by_social_post")) => {
			errors.push("consumed_by_social_post is required when status is consumed".into())
		},
		Some("canceled" | "expired") if !is_non_empty_string(entry.get("release_reason")) => {
			errors.push("release_reason is required when status is canceled or expired".into())
		},
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

	for (index, item) in items.iter().enumerate() {
		let Some(text) = item.as_str() else {
			errors.push(format!("text[{index}] must be a string"));

			continue;
		};

		validate_social_post_text_item(text, index, errors);
	}
}

fn validate_social_post_text_item(text: &str, index: usize, errors: &mut Vec<String>) {
	if text.is_empty() || text.len() > 280 {
		errors.push(format!("text[{index}] must be a non-empty X-sized string"));
	}
	if text.contains("Automated by @hackink") {
		errors.push(format!("text[{index}] must not include automation attribution"));
	}
	if text.len() > 260 && !text.contains("https://") {
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
	.any(|field| non_empty_array(refs.get(*field)).is_some());

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
	let Some(claims) = claims.and_then(Value::as_array) else {
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

	if decision.get("daily_limit").and_then(Value::as_i64) != Some(8) {
		errors.push("decision.daily_limit must be 8".into());
	}

	validate_social_post_decision_counts(entry, decision, errors);
}

fn validate_social_post_decision_counts(
	entry: &Map<String, Value>,
	decision: &Map<String, Value>,
	errors: &mut Vec<String>,
) {
	for field in ["daily_count_before", "daily_count_after"] {
		if decision.get(field).and_then(Value::as_i64).is_none_or(|value| value < 0) {
			errors.push(format!("decision.{field} must be a non-negative integer"));
		}
	}

	let before = decision.get("daily_count_before").and_then(Value::as_i64);
	let after = decision.get("daily_count_after").and_then(Value::as_i64);
	let post_count = entry.get("text").and_then(Value::as_array).map_or(0, Vec::len) as i64;

	if let (Some(before), Some(after)) = (before, after) {
		if string_field(entry, "status") == Some("published") && after != before + post_count {
			errors.push("decision.daily_count_after must add the published post count".into());
		}
		if string_field(entry, "status") != Some("published") && after != before {
			errors.push("decision.daily_count_after must remain unchanged unless published".into());
		}
	}
}

fn validate_social_post_status_payload(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	match string_field(entry, "status") {
		Some("published") => validate_social_post_publication(entry.get("publication"), errors),
		Some("blocked") => validate_social_post_block(entry, errors),
		Some("failed") if entry.get("failure").and_then(Value::as_object).is_none() => {
			errors.push("failure is required when status is failed".into())
		},
		Some("skipped") if entry.get("skip").and_then(Value::as_object).is_none() => {
			errors.push("skip is required when status is skipped".into())
		},
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
	if lifecycle.get("quote_eligible").and_then(Value::as_bool).is_none() {
		errors.push("post_lifecycle.quote_eligible must be boolean".into());
	}
	if !is_non_empty_string(lifecycle.get("reason")) {
		errors.push("post_lifecycle.reason must be a non-empty string".into());
	}
	if lifecycle
		.get("superseded_by_candidate")
		.is_some_and(|value| !is_non_empty_string(Some(value)))
	{
		errors.push("post_lifecycle.superseded_by_candidate must be non-empty when present".into());
	}

	let current_state = string_field(lifecycle, "current_state");
	let quote_eligible = lifecycle.get("quote_eligible").and_then(Value::as_bool);

	if quote_eligible == Some(true)
		&& (string_field(entry, "status") != Some("published") || current_state != Some("live"))
	{
		errors
			.push("post_lifecycle.quote_eligible can be true only for live published posts".into());
	}
	if current_state.is_some_and(|state| state.starts_with("superseded"))
		&& lifecycle.get("superseded_by_candidate").is_none()
	{
		errors.push(
			"post_lifecycle.superseded_by_candidate is required for superseded states".into(),
		);
	}
}

fn validate_social_post_publication(publication: Option<&Value>, errors: &mut Vec<String>) {
	let Some(publication) = publication.and_then(Value::as_object) else {
		errors.push("publication is required when status is published".into());

		return;
	};

	if !matches_one_of(publication.get("publisher"), &["chrome", "x_api"]) {
		errors.push("publication.publisher must be chrome or x_api".into());
	}
	if publication.get("account_verified").and_then(Value::as_bool) != Some(true) {
		errors.push("publication.account_verified must be true".into());
	}
	if publication.get("made_with_ai").and_then(Value::as_bool).is_none() {
		errors.push("publication.made_with_ai must be boolean".into());
	}
	if publication.get("image_template").is_some()
		&& string_field(publication, "image_template") != Some("decodex_signal_card")
	{
		errors.push("publication.image_template must be decodex_signal_card when present".into());
	}
	if !non_empty_array(publication.get("published_urls"))
		.is_some_and(|urls| urls.iter().all(|url| is_https_string(Some(url))))
	{
		errors.push("publication.published_urls must be a non-empty list of https URLs".into());
	}
	if !is_non_empty_string(publication.get("posted_at")) {
		errors.push("publication.posted_at must be a non-empty string".into());
	}
}

fn validate_social_post_block(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	let Some(block) = entry.get("block").and_then(Value::as_object) else {
		errors.push("block is required when status is blocked".into());

		return;
	};

	if !matches_one_of(block.get("reason"), SOCIAL_BLOCK_REASONS) {
		errors.push(format!("block.reason must be one of {}", choices(SOCIAL_BLOCK_REASONS)));
	}

	let count_before = entry
		.get("decision")
		.and_then(Value::as_object)
		.and_then(|decision| decision.get("daily_count_before"))
		.and_then(Value::as_i64);

	if string_field(block, "reason") == Some("daily_cap_exceeded")
		&& count_before.is_none_or(|count| count < 8)
	{
		errors.push("daily_cap_exceeded requires decision.daily_count_before >= 8".into());
	}
	if !is_non_empty_string(block.get("operator_notice")) {
		errors.push("block.operator_notice must be a non-empty string".into());
	}
}

pub(super) fn validate_signal_slug_uniqueness(
	path: &Path,
	payload: &Value,
	state: &mut ValidationState,
	errors: &mut Vec<String>,
) {
	let Some(slug) = payload.get("slug").and_then(Value::as_str) else {
		return;
	};

	if let Some(existing) = state.seen_signal_slugs.insert(slug.to_owned(), path.to_path_buf()) {
		errors.push(format!(
			"{}: duplicate slug {slug:?} also used by {}",
			path.display(),
			existing.display()
		));
	}
}

pub(super) fn validate_terminal_social_post_idempotency_key_uniqueness(
	path: &Path,
	payload: &Value,
	state: &mut ValidationState,
	errors: &mut Vec<String>,
) {
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
		state.seen_terminal_social_post_idempotency_keys.insert(key.to_owned(), path.to_path_buf())
	{
		errors.push(format!(
			"{}: duplicate terminal social_post idempotency_key {key:?} also used by {}",
			path.display(),
			existing.display()
		));
	}
	if let Some(existing) = state.active_social_publish_reservation_idempotency_keys.get(key) {
		errors.push(format!(
			"{}: terminal social_post idempotency_key {key:?} conflicts with active reservation {}",
			path.display(),
			existing.display()
		));
	}
}

pub(super) fn validate_active_social_publish_reservation_uniqueness(
	path: &Path,
	payload: &Value,
	state: &mut ValidationState,
	errors: &mut Vec<String>,
) {
	if payload.get("status").and_then(Value::as_str) != Some("active") {
		return;
	}

	let Some(key) = payload.get("idempotency_key").and_then(Value::as_str) else {
		return;
	};

	if let Some(existing) = state.seen_terminal_social_post_idempotency_keys.get(key) {
		errors.push(format!(
			"{}: active social_publish_reservation idempotency_key {key:?} conflicts with terminal social_post {}",
			path.display(),
			existing.display()
		));
	}
	if let Some(existing) = state
		.active_social_publish_reservation_idempotency_keys
		.insert(key.to_owned(), path.to_path_buf())
	{
		errors.push(format!(
			"{}: duplicate active social_publish_reservation idempotency_key {key:?} also used by {}",
			path.display(),
			existing.display()
		));
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

fn validate_string_list(value: Option<&Value>, label: &str, errors: &mut Vec<String>) {
	let valid = value.and_then(Value::as_array).is_some_and(|values| {
		values.iter().all(|item| item.as_str().is_some_and(|item| !item.is_empty()))
	});

	if !valid {
		errors.push(format!("{label} must be a list"));
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

fn validate_optional_positive_integer_list(
	value: Option<&Value>,
	label: &str,
	errors: &mut Vec<String>,
) {
	let Some(value) = value else {
		return;
	};

	if value.is_null() {
		return;
	}
	if !value
		.as_array()
		.is_some_and(|values| values.iter().all(|item| item.as_i64().is_some_and(|item| item > 0)))
	{
		errors.push(format!("{label} must be a list of positive integers"));
	}
}

fn optional_array<'a>(
	value: Option<&'a Value>,
	label: &str,
	errors: &mut Vec<String>,
) -> Option<&'a Vec<Value>> {
	match value {
		Some(Value::Array(values)) => Some(values),
		Some(Value::Null) | None => None,
		Some(_) => {
			errors.push(format!("{label} must be a list when present"));

			None
		},
	}
}

fn string_field<'a>(object: &'a Map<String, Value>, field: &str) -> Option<&'a str> {
	object.get(field).and_then(Value::as_str)
}

fn is_non_empty_string(value: Option<&Value>) -> bool {
	value.and_then(Value::as_str).is_some_and(|value| !value.is_empty())
}

fn is_truthy_json_value(value: Option<&Value>) -> bool {
	match value {
		Some(Value::Null) | None => false,
		Some(Value::String(value)) => !value.is_empty(),
		Some(_) => true,
	}
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

fn is_sha256_hex(value: &str) -> bool {
	value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn is_https_string_array(value: &Value) -> bool {
	value.as_array().is_some_and(|values| values.iter().all(|url| is_https_string(Some(url))))
}

fn choices(values: &[&str]) -> String {
	let quoted = values.iter().map(|value| format!("'{value}'")).collect::<Vec<_>>().join(", ");

	format!("[{quoted}]")
}

fn known_schemas() -> String {
	choices(&[
		BUNDLE_SCHEMA,
		CONFIG_FEATURE_CATALOG_SCHEMA,
		CONTROL_PLANE_UPGRADE_CANDIDATE_SCHEMA,
		RADAR_ARCHIVE_MANIFEST_SCHEMA,
		RELEASE_DELTA_SCHEMA,
		SIGNAL_SCHEMA,
		SOCIAL_CANDIDATE_SCHEMA,
		SOCIAL_POST_SCHEMA,
		SOCIAL_PUBLISH_RESERVATION_SCHEMA,
		UPSTREAM_IMPACT_SCHEMA,
		UPSTREAM_REVIEW_QUEUE_SCHEMA,
		UPSTREAM_REVIEW_SCHEMA,
	])
}
