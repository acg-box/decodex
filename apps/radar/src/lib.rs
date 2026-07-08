//! Radar auxiliary automation and artifact tooling.

mod artifact_validation;
mod cli;
mod constants;
mod core_io;
mod github_api;
mod github_bundle_client;
mod github_token;
mod ledger;
mod operations;
mod paths;
mod release_delta;
mod requests;
mod review_queue;
mod signal_render;
mod source_bundle;
mod text_values;
mod validation_files;
mod prelude {
	pub use color_eyre::{Result, eyre};
}

pub(crate) use self::{
	constants::{
		ANALYSIS_DRAFT_KIND, ARTIFACT_KINDS, ATTENTION_RULES, BUNDLE_SCHEMA,
		CONFIG_FEATURE_CATALOG_PATH, CONFIG_FEATURE_CATALOG_SCHEMA,
		CONTROL_PLANE_UPGRADE_CANDIDATE_SCHEMA, DEFAULT_LEDGER_PATH, DEFAULT_MIN_STABLE_TAG,
		DEFAULT_PAIR_LIMIT, DEFAULT_PREVIEW_LIMIT, DEFAULT_QUEUE_OUT, DEFAULT_RELEASE_DELTA_OUT,
		DEFAULT_SEARCH_LIMIT, DEFAULT_SIGNALS_DIR, DEFAULT_STABLE_LIMIT, DEFAULT_TAG_PREFIX,
		DEFAULT_VALIDATION_PATHS, GENERIC_COMMIT_TITLES, GITHUB_REQUEST_ATTEMPTS,
		GITHUB_REQUEST_BACKOFF, GITHUB_REQUEST_TIMEOUT, HIGH_VALUE_SURFACES,
		RADAR_ARCHIVE_HISTORICAL_RETENTION_CUTOFF, RELEASE_DELTA_SCHEMA,
		RETRYABLE_GITHUB_STATUS_CODES, REVIEW_STATUSES, RUN_CODEX_ANALYSIS_SCRIPT, SCHEMA_VERSION,
		SIGNAL_CONFIDENCE, SIGNAL_SCHEMA, SURFACE_RULES, UPSTREAM_IMPACT_SCHEMA,
		UPSTREAM_REVIEW_LINEAR_FOLLOWUP_CUTOFF, UPSTREAM_REVIEW_QUEUE_SCHEMA,
		UPSTREAM_REVIEW_SCHEMA, UPSTREAM_SUBJECT_KINDS,
	},
	core_io::{
		absolute_repo_path, collect_bundle_json_files, ledger_path, load_known_feature_names,
		repo_default_branch, sorted_json_files, validate_expected_schema,
		write_json_if_material_changed,
	},
	ledger::{
		default_ledger_path, ledger_artifact_link, ledger_bootstrap, ledger_ingest,
		ledger_ingest_existing, ledger_summary,
	},
	operations::{build_bundle, refresh_queue, render_signal, validate, validate_bundles},
	release_delta::{backfill_release_range, refresh_release_delta},
	requests::{
		RadarBackfillReleaseRangeReport, RadarBackfillReleaseRangeRequest, RadarBundleBuildRequest,
		RadarBundleValidateRequest, RadarLedgerArtifactLinkRequest, RadarLedgerBootstrapRequest,
		RadarLedgerIngestExistingRequest, RadarLedgerIngestRequest, RadarLedgerSummaryRequest,
		RadarRefreshQueueReport, RadarRefreshQueueRequest, RadarRefreshReleaseDeltaReport,
		RadarRefreshReleaseDeltaRequest, RadarRenderSignalReport, RadarRenderSignalRequest,
		RadarValidateRequest, RadarValidationReport,
	},
	text_values::{
		body_excerpt, extract_commit_sha_from_url, extract_pr_number_from_url,
		optional_value_string, path_arg, percent_encode, pretty_json, repo_root,
		required_value_i64, required_value_string, required_value_u64, resolve_against, short_sha,
		slugify, string_array, string_array_from_value, truncate_patch_excerpt,
	},
	validation_files::{
		collect_json_files, first_line, is_truthy_json_value, load_json, non_empty_array,
		object_value, optional_string, queue_report, require_member, required_string, string_field,
		utc_now_iso, validation_paths, write_json,
	},
};

use std::{
	collections::{BTreeMap, BTreeSet, HashSet},
	fs::{self, OpenOptions},
	io::Write,
	iter,
	path::{Path, PathBuf},
	process,
};

use clap::Parser as _;
use serde_json::{self, Map, Value};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::prelude::eyre;
#[cfg(test)] use artifact_validation::has_legacy_multi_agent_v2_context;
use artifact_validation::{
	ValidationState, validate_analysis_draft, validate_artifact, validate_artifact_errors,
	validate_artifact_for_path, validate_signal_file, validate_signal_slug_uniqueness,
};
use cli::Cli;
use github_api::GitHubApi;
use github_bundle_client::GithubClient;
use github_token::github_token;
use ledger::RadarLedger;
use prelude::Result;
use review_queue::{RecentCommit, build_review_queue};
use signal_render::{rendered_config_flags, rendered_signal};
use source_bundle::{build_commit_bundle_from_sources, build_pr_bundle_from_sources};

#[derive(Debug)]
enum RefreshKind {
	Queue,
	ReleaseDelta,
}

/// Run the Radar CLI.
pub fn run() -> Result<()> {
	color_eyre::install()?;

	Cli::parse().run()
}

#[cfg(test)] mod test_support;
#[cfg(test)] mod tests;
