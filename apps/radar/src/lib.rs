//! Radar auxiliary automation and artifact tooling.

use std::{
	collections::{BTreeMap, BTreeSet, HashSet},
	env,
	fs::{self, OpenOptions},
	io::Write as _,
	iter,
	path::{Path, PathBuf},
	process,
	sync::OnceLock,
	time::Duration,
};

use regex::Regex;
use reqwest::StatusCode;
use serde_json::{self, Map, Value};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::prelude::eyre;

mod artifact_validation;
mod cli;
mod config;
mod github_api;
mod github_bundle_client;
mod github_token;
mod ledger;
mod paths;
mod release_delta;
mod requests;
mod review_queue;
mod signal_render;
mod source_bundle;

mod prelude {
	pub use color_eyre::{Result, eyre};
}

#[cfg(test)]
mod test_support {
	use std::sync::{Mutex, MutexGuard, OnceLock};

	pub(crate) struct TestEnvLockGuard {
		_lock: MutexGuard<'static, ()>,
	}

	pub(crate) fn lock_test_env() -> TestEnvLockGuard {
		TestEnvLockGuard {
			_lock: test_env_mutex().lock().expect("test env mutex should not be poisoned"),
		}
	}

	fn test_env_mutex() -> &'static Mutex<()> {
		static TEST_ENV_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

		TEST_ENV_MUTEX.get_or_init(|| Mutex::new(()))
	}
}

/// Run the Radar CLI.
pub fn run() -> prelude::Result<()> {
	use clap::Parser as _;

	color_eyre::install()?;

	cli::Cli::parse().run()
}

#[cfg(test)] use artifact_validation::has_legacy_multi_agent_v2_context;
use artifact_validation::{
	ValidationState, validate_analysis_draft, validate_artifact, validate_artifact_errors,
	validate_artifact_for_path, validate_signal_file, validate_signal_slug_uniqueness,
};
use github_api::GitHubApi;
use github_bundle_client::GithubClient;
use github_token::github_token;
use ledger::RadarLedger;
pub(crate) use release_delta::{backfill_release_range, refresh_release_delta};
use review_queue::{RecentCommit, build_review_queue};
use signal_render::{rendered_config_flags, rendered_signal};
use source_bundle::{build_commit_bundle_from_sources, build_pr_bundle_from_sources};

pub(crate) use ledger::{
	default_ledger_path, ledger_artifact_link, ledger_bootstrap, ledger_ingest,
	ledger_ingest_existing, ledger_summary,
};

pub(crate) use requests::{
	RadarBackfillReleaseRangeReport, RadarBackfillReleaseRangeRequest, RadarBundleBuildRequest,
	RadarBundleValidateRequest, RadarLedgerArtifactLinkRequest, RadarLedgerBootstrapRequest,
	RadarLedgerIngestExistingRequest, RadarLedgerIngestRequest, RadarLedgerSummaryRequest,
	RadarRefreshQueueReport, RadarRefreshQueueRequest, RadarRefreshReleaseDeltaReport,
	RadarRefreshReleaseDeltaRequest, RadarRenderSignalReport, RadarRenderSignalRequest,
	RadarValidateRequest, RadarValidationReport,
};

const BUNDLE_SCHEMA: &str = "github_change_bundle/v1";
const CONTROL_PLANE_UPGRADE_CANDIDATE_SCHEMA: &str = "control_plane_upgrade_candidate/v1";
const DEFAULT_LEDGER_PATH: &str = paths::DEFAULT_LEDGER_PATH;
const DEFAULT_MIN_STABLE_TAG: &str = "rust-v0.116.0";
const DEFAULT_PAIR_LIMIT: usize = 24;
const DEFAULT_PREVIEW_LIMIT: usize = 0;
const DEFAULT_QUEUE_OUT: &str = paths::DEFAULT_QUEUE_OUT;
const DEFAULT_RELEASE_DELTA_OUT: &str = paths::DEFAULT_RELEASE_DELTA_OUT;
const DEFAULT_SEARCH_LIMIT: usize = 40;
const DEFAULT_SIGNALS_DIR: &str = paths::DEFAULT_SIGNALS_DIR;
const DEFAULT_STABLE_LIMIT: usize = 0;
const DEFAULT_TAG_PREFIX: &str = "rust-v";
const RELEASE_DELTA_SCHEMA: &str = "release_delta/v1";
const SCHEMA_VERSION: i64 = 5;
const SIGNAL_SCHEMA: &str = "signal_entry/v1";
const UPSTREAM_IMPACT_SCHEMA: &str = "upstream_impact/v1";
const UPSTREAM_REVIEW_QUEUE_SCHEMA: &str = "upstream_review_queue/v1";
const UPSTREAM_REVIEW_SCHEMA: &str = "upstream_review/v1";
const CONFIG_FEATURE_CATALOG_SCHEMA: &str = "codex_config_feature_catalog/v1";
const ANALYSIS_DRAFT_KIND: &str = "analysis_draft";
const RADAR_ARCHIVE_HISTORICAL_RETENTION_CUTOFF: &str = "2026-06-07T00:00:00Z";
const UPSTREAM_REVIEW_LINEAR_FOLLOWUP_CUTOFF: &str = "2026-06-12T00:00:00Z";
const SIGNAL_CONFIDENCE: &[&str] = &["confirmed", "likely", "weak"];
const UPSTREAM_SUBJECT_KINDS: &[&str] = &["commit", "pr"];
const DEFAULT_VALIDATION_PATHS: &[&str] = paths::DEFAULT_VALIDATION_PATHS;
const GENERIC_COMMIT_TITLES: &[&str] =
	&["update", "fix", "fix.", "fix tests", "fix tests.", "merge fixes", "flaky syntax"];
const CONFIG_FEATURE_CATALOG_PATH: &str = paths::CONFIG_FEATURE_CATALOG_PATH;
const RUN_CODEX_ANALYSIS_SCRIPT: &str = paths::RUN_CODEX_ANALYSIS_SCRIPT;
const HIGH_VALUE_SURFACES: &[&str] = &[
	"app_server_protocol",
	"mcp_plugins",
	"browser_chrome",
	"sandbox_permissions",
	"config_hooks",
	"auth_accounts",
	"model_provider",
];
const ATTENTION_RULES: &[(&str, &[&str])] = &[
	(
		"new_feature",
		&["feat", "feature", "add ", "adds ", "support", "enable", "implement", "introduce"],
	),
	("deprecated_removed", &["deprecat", "remove", "removed", "delete", "disable", "no longer"]),
	(
		"protocol_change",
		&[
			"protocol",
			"schema",
			"api",
			"json-rpc",
			"jsonrpc",
			"notification",
			"request",
			"response",
		],
	),
	("breaking_change", &["breaking", "break ", "rename", "migration", "incompat", "no longer"]),
	(
		"security_policy",
		&["sandbox", "permission", "approval", "full access", "network", "denylist", "allowlist"],
	),
	("rate_limit", &["rate limit", "ratelimit", "quota", "usage limit", "message cap"]),
	("auth_account", &["auth", "account", "login", "token"]),
	("release_packaging", &["release", "appcast", "sparkle", "beta", "version"]),
];
const SURFACE_RULES: &[(&str, &[&str])] = &[
	("app_server_protocol", &["app-server", "app_server", "protocol", "jsonrpc", "json-rpc"]),
	("mcp_plugins", &["mcp", "plugin", "tool-search", "tool_search"]),
	("browser_chrome", &["browser", "chrome", "webview"]),
	(
		"sandbox_permissions",
		&["sandbox", "permission", "approval", "policy", "denylist", "allowlist"],
	),
	("config_hooks", &["config", "hook", "settings", "toml"]),
	("auth_accounts", &["auth", "account", "login", "token"]),
	("model_provider", &["model", "provider", "rate-limit", "ratelimit", "quota"]),
	("cli_tui", &["cli", "tui", "terminal", "chatwidget"]),
	("release_packaging", &["release", "appcast", "sparkle", "version", "install", "package"]),
	("docs_examples", &["docs/", "readme", "example"]),
	("tests_ci", &["test", "tests", ".github", "ci", "fixture"]),
];
const REVIEW_STATUSES: &[&str] =
	&["archived", "control_plane", "deprecated", "seen", "signal", "skipped", "watch"];
const ARTIFACT_KINDS: &[&str] = &[
	"analysis",
	"archive_manifest",
	"bundle",
	"control_plane_upgrade_candidate",
	"ledger_export",
	"release_delta",
	"signal",
	"upstream_impact",
];
const GITHUB_REQUEST_ATTEMPTS: usize = 4;
const GITHUB_REQUEST_BACKOFF: Duration = Duration::from_secs(1);
const GITHUB_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const RETRYABLE_GITHUB_STATUS_CODES: &[StatusCode] = &[
	StatusCode::TOO_MANY_REQUESTS,
	StatusCode::INTERNAL_SERVER_ERROR,
	StatusCode::BAD_GATEWAY,
	StatusCode::SERVICE_UNAVAILABLE,
	StatusCode::GATEWAY_TIMEOUT,
];

#[derive(Debug)]
enum RefreshKind {
	Queue,
	ReleaseDelta,
}
mod core_io;
mod operations;
mod text_values;
mod validation_files;

#[allow(clippy::wildcard_imports)]
pub(crate) use self::{core_io::*, operations::*, text_values::*, validation_files::*};

#[cfg(test)] mod tests;
