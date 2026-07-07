use std::path::Path;

use serde_json::Value;

use crate::artifact_validation::{
	ANALYSIS_DRAFT_KIND, BUNDLE_SCHEMA, CONFIG_FEATURE_CATALOG_SCHEMA,
	CONTROL_PLANE_UPGRADE_CANDIDATE_SCHEMA, RELEASE_DELTA_SCHEMA, SIGNAL_SCHEMA,
	UPSTREAM_IMPACT_SCHEMA, UPSTREAM_REVIEW_QUEUE_SCHEMA, UPSTREAM_REVIEW_SCHEMA, archive, bundle,
	constants::RADAR_ARCHIVE_MANIFEST_SCHEMA,
	core::{analysis, paths},
	model::{ArtifactValidation, ArtifactValidationOptions},
	release, signal, support, upstream,
};

pub(crate) fn validate_artifact_errors(payload: &Value) -> Vec<String> {
	validate_artifact(payload).errors
}

pub(crate) fn validate_artifact(payload: &Value) -> ArtifactValidation {
	validate_artifact_with_options(payload, ArtifactValidationOptions::default())
}

pub(crate) fn validate_artifact_for_path(path: &Path, payload: &Value) -> ArtifactValidation {
	if paths::is_analysis_draft_path(path) && payload.get("schema").is_none() {
		return match analysis::validate_analysis_draft(payload) {
			Ok(()) => {
				ArtifactValidation { schema: Some(ANALYSIS_DRAFT_KIND.into()), errors: Vec::new() }
			},
			Err(error) => ArtifactValidation {
				schema: Some(ANALYSIS_DRAFT_KIND.into()),
				errors: paths::analysis_draft_error_lines(error),
			},
		};
	}

	validate_artifact_with_options(
		payload,
		ArtifactValidationOptions {
			allow_historical_archive_retention: paths::is_historical_archive_manifest_path(
				path, payload,
			),
			allow_historical_upstream_review_linear_followup:
				paths::is_historical_upstream_review_path(path, payload),
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
		Some(BUNDLE_SCHEMA) => bundle::validate_bundle(entry, &mut errors),
		Some(CONFIG_FEATURE_CATALOG_SCHEMA) => {
			signal::validate_config_feature_catalog(entry, &mut errors)
		},
		Some(CONTROL_PLANE_UPGRADE_CANDIDATE_SCHEMA) => {
			upstream::validate_control_plane_upgrade_candidate(entry, &mut errors)
		},
		Some(RADAR_ARCHIVE_MANIFEST_SCHEMA) => {
			archive::validate_radar_archive_manifest(entry, options, &mut errors)
		},
		Some(RELEASE_DELTA_SCHEMA) => release::validate_release_delta(entry, &mut errors),
		Some(SIGNAL_SCHEMA) => signal::validate_signal(entry, &mut errors),
		Some(UPSTREAM_IMPACT_SCHEMA) => upstream::validate_upstream_impact(entry, &mut errors),
		Some(UPSTREAM_REVIEW_QUEUE_SCHEMA) => {
			upstream::validate_upstream_review_queue(entry, &mut errors)
		},
		Some(UPSTREAM_REVIEW_SCHEMA) => {
			upstream::validate_upstream_review(entry, options, &mut errors)
		},
		Some(_) | None => {
			errors.push(format!("schema must be one of {}", support::known_schemas()))
		},
	}

	ArtifactValidation { schema, errors }
}
