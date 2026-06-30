//! Radar artifact validation facade.

mod archive;
mod bundle;
mod constants;
mod core;
mod model;
mod release;
mod signal;
mod support;
mod uniqueness;
mod upstream;

pub(super) use self::{
	core::{
		validate_analysis_draft, validate_artifact, validate_artifact_errors,
		validate_artifact_for_path, validate_signal_file,
	},
	model::ValidationState,
	uniqueness::validate_signal_slug_uniqueness,
};

#[cfg(test)] pub(super) use self::signal::has_legacy_multi_agent_v2_context;

use self::constants::RADAR_ARCHIVE_MANIFEST_SCHEMA;
use super::{
	ANALYSIS_DRAFT_KIND, BUNDLE_SCHEMA, CONFIG_FEATURE_CATALOG_SCHEMA,
	CONTROL_PLANE_UPGRADE_CANDIDATE_SCHEMA, RADAR_ARCHIVE_HISTORICAL_RETENTION_CUTOFF,
	RELEASE_DELTA_SCHEMA, SIGNAL_CONFIDENCE, SIGNAL_SCHEMA, UPSTREAM_IMPACT_SCHEMA,
	UPSTREAM_REVIEW_LINEAR_FOLLOWUP_CUTOFF, UPSTREAM_REVIEW_QUEUE_SCHEMA, UPSTREAM_REVIEW_SCHEMA,
	UPSTREAM_SUBJECT_KINDS,
};
