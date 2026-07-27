//! Radar auxiliary automation and artifact tooling.

mod artifact_validation;
mod cache;
mod cli;
mod constants;
mod content_eligibility;
mod content_review;
mod core_io;
mod github_api;
mod github_bundle_client;
mod github_token;
mod ledger;
mod operations;
mod paths;
mod private_fs;
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
	cache::cache_gc,
	constants::{
		ANALYSIS_DRAFT_KIND, ARTIFACT_KINDS, ATTENTION_RULES, BUNDLE_SCHEMA, CACHE_MAX_AGE_DAYS,
		CACHE_MAX_BYTES_PER_COLLECTION, CACHE_MAX_FILES_PER_COLLECTION,
		CONFIG_FEATURE_CATALOG_PATH, CONFIG_FEATURE_CATALOG_SCHEMA,
		CONTROL_PLANE_UPGRADE_CANDIDATE_SCHEMA, DEFAULT_CACHE_ROOT, DEFAULT_LEDGER_PATH,
		DEFAULT_MIN_STABLE_TAG, DEFAULT_PAIR_LIMIT, DEFAULT_PREVIEW_LIMIT, DEFAULT_QUEUE_OUT,
		DEFAULT_RELEASE_DELTA_OUT, DEFAULT_SEARCH_LIMIT, DEFAULT_SIGNALS_DIR,
		DEFAULT_SOURCE_MAX_AGE_HOURS, DEFAULT_STABLE_LIMIT, DEFAULT_TAG_PREFIX,
		DEFAULT_VALIDATION_PATHS, GENERIC_COMMIT_TITLES, GITHUB_REQUEST_ATTEMPTS,
		GITHUB_REQUEST_BACKOFF, GITHUB_REQUEST_TIMEOUT, HIGH_VALUE_SURFACES, LEDGER_MAX_BYTES,
		LEDGER_MAX_ROWS_PER_TABLE, RELEASE_DELTA_SCHEMA, RETAINED_CACHE_COLLECTIONS,
		RETRYABLE_GITHUB_STATUS_CODES, REVIEW_STATUSES, RUN_CODEX_ANALYSIS_SCRIPT, SCHEMA_VERSION,
		SIGNAL_CONFIDENCE, SIGNAL_SCHEMA, SURFACE_RULES, UPSTREAM_IMPACT_SCHEMA,
		UPSTREAM_REVIEW_QUEUE_SCHEMA, UPSTREAM_REVIEW_SCHEMA, UPSTREAM_SUBJECT_KINDS,
	},
	content_eligibility::content_eligibility,
	content_review::review_next,
	core_io::{
		RefreshWriteReport, absolute_repo_path, collect_bundle_json_files, inspect_json_refresh,
		ledger_path, load_known_feature_names, refresh_json, repo_default_branch,
		sorted_json_files, validate_expected_schema,
	},
	ledger::{
		default_ledger_path, ledger_artifact_link, ledger_bootstrap, ledger_ingest,
		ledger_ingest_existing, ledger_summary,
	},
	operations::{build_bundle, refresh_queue, render_signal, validate, validate_bundles},
	private_fs::{
		collect_private_json_files, collect_private_json_files_if_present, is_radar_cache_path,
		private_file_exists, read_private_file, read_private_files, write_private_file_atomic,
	},
	release_delta::{backfill_release_range, refresh_release_delta},
	requests::{
		RadarBackfillReleaseRangeReport, RadarBackfillReleaseRangeRequest, RadarBundleBuildRequest,
		RadarBundleValidateRequest, RadarCacheGcReport, RadarCacheGcRequest,
		RadarContentEligibilityReport, RadarContentEligibilityRequest,
		RadarLedgerArtifactLinkRequest, RadarLedgerBootstrapRequest,
		RadarLedgerIngestExistingRequest, RadarLedgerIngestRequest, RadarLedgerSummaryRequest,
		RadarQueueGeneration, RadarRefreshQueueReport, RadarRefreshQueueRequest,
		RadarRefreshReleaseDeltaReport, RadarRefreshReleaseDeltaRequest, RadarRenderSignalReport,
		RadarRenderSignalRequest, RadarReviewNextReport, RadarReviewNextRequest,
		RadarSelectedSubject, RadarSourceRef, RadarValidateRequest, RadarValidationReport,
	},
	text_values::{
		body_excerpt, extract_commit_sha_from_url, extract_pr_number_from_url,
		optional_value_string, path_arg, percent_encode, pretty_json, repo_root,
		required_value_i64, required_value_string, required_value_u64, resolve_against, short_sha,
		slugify, string_array, string_array_from_value, truncate_patch_excerpt,
	},
	validation_files::{
		collect_json_files, first_line, is_default_source_snapshot, is_truthy_json_value,
		load_json, non_empty_array, object_value, optional_string, queue_report, require_member,
		required_string, string_field, utc_now_iso, validate_source_freshness, validation_paths,
		write_json,
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
#[cfg(test)] use operations::validate_default_cache_presence;
use prelude::Result;
#[cfg(test)] use private_fs::simulate_wrong_owner_error;
#[cfg(test)] use private_fs::{create_private_file, ensure_private_directory};
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
