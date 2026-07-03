//! Public entrypoints and dispatch for Radar artifact validation.

use std::path::Path;

use serde_json::{Map, Value};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	artifact_validation::{
		ANALYSIS_DRAFT_KIND, BUNDLE_SCHEMA, CONFIG_FEATURE_CATALOG_SCHEMA,
		CONTROL_PLANE_UPGRADE_CANDIDATE_SCHEMA, RADAR_ARCHIVE_HISTORICAL_RETENTION_CUTOFF,
		RELEASE_DELTA_SCHEMA, SIGNAL_CONFIDENCE, SIGNAL_SCHEMA, UPSTREAM_IMPACT_SCHEMA,
		UPSTREAM_REVIEW_LINEAR_FOLLOWUP_CUTOFF, UPSTREAM_REVIEW_QUEUE_SCHEMA,
		UPSTREAM_REVIEW_SCHEMA, archive, bundle,
		constants::{RADAR_ARCHIVE_MANIFEST_SCHEMA, SIGNAL_IMPACT, SIGNAL_KINDS},
		model::{ArtifactValidation, ArtifactValidationOptions},
		release,
		signal::{self},
		support,
		upstream::{self},
	},
	prelude::{
		Result,
		eyre::{self, Report},
	},
};

pub(crate) fn validate_signal_file(path: &Path, payload: &Value) -> Result<()> {
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

pub(crate) fn validate_analysis_draft(value: &Value) -> Result<()> {
	let Some(draft) = value.as_object() else {
		return Err(eyre::eyre!("Analysis draft must be an object"));
	};
	let mut errors = Vec::new();

	for field in ["kind", "title", "summary", "why_it_matters", "confidence", "impact"] {
		if !support::is_non_empty_string(draft.get(field)) {
			errors.push(format!("{field} is required in analysis draft"));
		}
	}

	if !support::matches_one_of(draft.get("kind"), SIGNAL_KINDS) {
		errors.push(format!("kind must be one of {}", support::choices(SIGNAL_KINDS)));
	}
	if !support::matches_one_of(draft.get("confidence"), SIGNAL_CONFIDENCE) {
		errors.push(format!("confidence must be one of {}", support::choices(SIGNAL_CONFIDENCE)));
	}
	if !support::matches_one_of(draft.get("impact"), SIGNAL_IMPACT) {
		errors.push(format!("impact must be one of {}", support::choices(SIGNAL_IMPACT)));
	}
	if support::non_empty_array(draft.get("proof_points")).is_none() {
		errors.push("proof_points must be a non-empty list".into());
	}
	if support::string_field(draft, "kind") == Some("try_now")
		&& !support::is_truthy_json_value(draft.get("how_to_try"))
	{
		errors.push("how_to_try is required when kind is try_now".into());
	}
	if support::is_truthy_json_value(draft.get("how_to_try"))
		&& !support::is_truthy_json_value(draft.get("expected_effect"))
	{
		errors.push("expected_effect is required when how_to_try is present".into());
	}
	if errors.is_empty() {
		Ok(())
	} else {
		Err(eyre::eyre!("Analysis draft validation failed:\n- {}", errors.join("\n- ")))
	}
}

pub(crate) fn validate_artifact_errors(payload: &Value) -> Vec<String> {
	validate_artifact(payload).errors
}

pub(crate) fn validate_artifact(payload: &Value) -> ArtifactValidation {
	validate_artifact_with_options(payload, ArtifactValidationOptions::default())
}

pub(crate) fn validate_artifact_for_path(path: &Path, payload: &Value) -> ArtifactValidation {
	if is_analysis_draft_path(path) && payload.get("schema").is_none() {
		return match validate_analysis_draft(payload) {
			Ok(()) =>
				ArtifactValidation { schema: Some(ANALYSIS_DRAFT_KIND.into()), errors: Vec::new() },
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

pub(crate) fn validate_artifact_with_options(
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
		Some(BUNDLE_SCHEMA) => bundle::validate_bundle(entry, &mut errors),
		Some(CONFIG_FEATURE_CATALOG_SCHEMA) =>
			signal::validate_config_feature_catalog(entry, &mut errors),
		Some(CONTROL_PLANE_UPGRADE_CANDIDATE_SCHEMA) =>
			upstream::validate_control_plane_upgrade_candidate(entry, &mut errors),
		Some(RADAR_ARCHIVE_MANIFEST_SCHEMA) =>
			archive::validate_radar_archive_manifest(entry, options, &mut errors),
		Some(RELEASE_DELTA_SCHEMA) => release::validate_release_delta(entry, &mut errors),
		Some(SIGNAL_SCHEMA) => signal::validate_signal(entry, &mut errors),
		Some(UPSTREAM_IMPACT_SCHEMA) => upstream::validate_upstream_impact(entry, &mut errors),
		Some(UPSTREAM_REVIEW_QUEUE_SCHEMA) =>
			upstream::validate_upstream_review_queue(entry, &mut errors),
		Some(UPSTREAM_REVIEW_SCHEMA) =>
			upstream::validate_upstream_review(entry, options, &mut errors),
		Some(_) | None =>
			errors.push(format!("schema must be one of {}", support::known_schemas())),
	}

	ArtifactValidation { schema, errors }
}

pub(super) fn is_analysis_draft_path(path: &Path) -> bool {
	let normalized = normalized_path(path);

	normalized.ends_with(".analysis.json")
		&& (normalized.contains("/generated/analysis/")
			|| normalized.starts_with("generated/analysis/"))
}

pub(super) fn is_historical_archive_manifest_path(path: &Path, payload: &Value) -> bool {
	let Some(entry) = payload.as_object() else {
		return false;
	};
	let normalized = normalized_path(path);

	support::string_field(entry, "schema") == Some(RADAR_ARCHIVE_MANIFEST_SCHEMA)
		&& normalized.contains("/cache/archive/index/")
		&& timestamp_field_before(entry, "created_at", RADAR_ARCHIVE_HISTORICAL_RETENTION_CUTOFF)
}

pub(super) fn is_historical_upstream_review_path(path: &Path, payload: &Value) -> bool {
	let Some(entry) = payload.as_object() else {
		return false;
	};
	let normalized = normalized_path(path);

	support::string_field(entry, "schema") == Some(UPSTREAM_REVIEW_SCHEMA)
		&& normalized.contains("/cache/github/reviews/")
		&& timestamp_field_before(entry, "reviewed_at", UPSTREAM_REVIEW_LINEAR_FOLLOWUP_CUTOFF)
}

pub(super) fn timestamp_field_before(
	entry: &Map<String, Value>,
	field: &str,
	cutoff: &str,
) -> bool {
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

pub(super) fn normalized_path(path: &Path) -> String {
	path.to_string_lossy().replace('\\', "/")
}

pub(super) fn analysis_draft_error_lines(error: Report) -> Vec<String> {
	error
		.to_string()
		.lines()
		.map(str::trim)
		.filter(|line| !line.is_empty())
		.map(|line| line.trim_start_matches("- ").to_owned())
		.collect()
}
