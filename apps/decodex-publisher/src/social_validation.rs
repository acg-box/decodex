//! Decodex social artifact validation.

mod candidate;
mod common;
mod cross_file;
mod outcome;
mod post;
mod reservation;
mod strategy;

pub(crate) use cross_file::SocialValidationState;

use std::path::Path;

use serde_json::{Map, Value};

use crate::{
	SOCIAL_CANDIDATE_SCHEMA, SOCIAL_OUTCOME_SCHEMA, SOCIAL_POST_SCHEMA,
	SOCIAL_PUBLISH_RESERVATION_SCHEMA, SOCIAL_STRATEGY_SCHEMA,
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
		Some(SOCIAL_CANDIDATE_SCHEMA) => candidate::validate_social_candidate(entry, &mut errors),
		Some(SOCIAL_OUTCOME_SCHEMA) => outcome::validate_social_outcome(entry, &mut errors),
		Some(SOCIAL_POST_SCHEMA) => post::validate_social_post(entry, &mut errors),
		Some(SOCIAL_PUBLISH_RESERVATION_SCHEMA) =>
			reservation::validate_social_publish_reservation(entry, &mut errors),
		Some(SOCIAL_STRATEGY_SCHEMA) => strategy::validate_social_strategy(entry, &mut errors),
		Some(_) | None => errors.push(format!(
			"schema must be one of {}",
			choices(&[
				SOCIAL_CANDIDATE_SCHEMA,
				SOCIAL_OUTCOME_SCHEMA,
				SOCIAL_POST_SCHEMA,
				SOCIAL_PUBLISH_RESERVATION_SCHEMA,
				SOCIAL_STRATEGY_SCHEMA
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
	cross_file::validate_social_cross_file_constraints(path, payload, state, errors);
}

fn validate_social_post_text(text: Option<&Value>, errors: &mut Vec<String>) {
	post::validate_social_post_text(text, errors);
}

fn validate_social_post_claims(claims: Option<&Value>, errors: &mut Vec<String>) {
	post::validate_social_post_claims(claims, errors);
}

fn validate_non_empty_string_list(value: Option<&Value>, label: &str, errors: &mut Vec<String>) {
	common::validate_non_empty_string_list(value, label, errors);
}

fn validate_exact_keys(
	object: &Map<String, Value>,
	label: &str,
	allowed: &[&str],
	errors: &mut Vec<String>,
) {
	common::validate_exact_keys(object, label, allowed, errors);
}

fn validate_optional_string_list(value: Option<&Value>, label: &str, errors: &mut Vec<String>) {
	common::validate_optional_string_list(value, label, errors);
}

fn validate_rfc3339_field(entry: &Map<String, Value>, field: &str, errors: &mut Vec<String>) {
	common::validate_rfc3339_field(entry, field, errors);
}

fn string_field<'a>(object: &'a Map<String, Value>, field: &str) -> Option<&'a str> {
	common::string_field(object, field)
}

fn is_non_empty_string(value: Option<&Value>) -> bool {
	common::is_non_empty_string(value)
}

fn matches_one_of(value: Option<&Value>, choices: &[&str]) -> bool {
	common::matches_one_of(value, choices)
}

fn non_empty_array(value: Option<&Value>) -> Option<&Vec<Value>> {
	common::non_empty_array(value)
}

fn is_empty_or_missing_array(value: Option<&Value>) -> bool {
	common::is_empty_or_missing_array(value)
}

fn is_https_string(value: Option<&Value>) -> bool {
	common::is_https_string(value)
}

fn is_https_string_array(value: &Value) -> bool {
	common::is_https_string_array(value)
}

fn choices(values: &[&str]) -> String {
	common::choices(values)
}
