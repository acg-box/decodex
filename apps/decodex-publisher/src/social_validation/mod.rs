//! Decodex social artifact validation.

use std::{collections::BTreeMap, path::Path};

use serde_json::{Map, Value};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	SOCIAL_CANDIDATE_SCHEMA, SOCIAL_POST_SCHEMA, SOCIAL_PUBLISH_RESERVATION_SCHEMA, path_arg,
	repo_root,
};

mod candidate;
mod common;
mod cross_file;
mod post;
mod reservation;

use candidate::validate_social_candidate;
#[allow(clippy::wildcard_imports)] use common::*;
pub(crate) use cross_file::{SocialValidationState, validate_social_cross_file_constraints};
use post::{validate_social_post, validate_social_post_claims, validate_social_post_text};
use reservation::validate_social_publish_reservation;

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
