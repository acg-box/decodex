//! Rust-owned Radar artifact contracts and file validation.

use std::{
	collections::{BTreeMap, BTreeSet, HashSet},
	env,
	fs::{self, OpenOptions},
	io::Write as _,
	iter,
	path::{Path, PathBuf},
	process::{self, Command},
	sync::OnceLock,
	time::Duration,
};

use regex::Regex;
use reqwest::StatusCode;
use serde_json::{self, Map, Value};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::prelude::eyre::{self, Report};

mod github_api;
mod github_bundle_client;
mod github_token;
mod ledger;
mod requests;
mod signal_render;

use github_api::GitHubApi;
use github_bundle_client::GithubClient;
use github_token::github_token;
use ledger::RadarLedger;
use signal_render::{rendered_config_flags, rendered_signal};

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
	RadarSocialReservePublishReport, RadarSocialReservePublishRequest, RadarValidateRequest,
	RadarValidationReport,
};

const BUNDLE_SCHEMA: &str = "github_change_bundle/v1";
const CONTROL_PLANE_UPGRADE_CANDIDATE_SCHEMA: &str = "control_plane_upgrade_candidate/v1";
const DEFAULT_LEDGER_PATH: &str = ".agent/automations/decodex/cache/github/radar.sqlite3";
const DEFAULT_MIN_STABLE_TAG: &str = "rust-v0.116.0";
const DEFAULT_PAIR_LIMIT: usize = 24;
const DEFAULT_PREVIEW_LIMIT: usize = 0;
const DEFAULT_QUEUE_OUT: &str =
	".agent/automations/decodex/cache/github/review-queue/openai-codex-latest.json";
const DEFAULT_RELEASE_DELTA_OUT: &str =
	".agent/automations/decodex/cache/site-content/release-deltas/openai-codex-latest.json";
const DEFAULT_SEARCH_LIMIT: usize = 40;
const DEFAULT_SIGNALS_DIR: &str = ".agent/automations/decodex/cache/site-content/signals";
const DEFAULT_STABLE_LIMIT: usize = 0;
const DEFAULT_TAG_PREFIX: &str = "rust-v";
const RELEASE_DELTA_SCHEMA: &str = "release_delta/v1";
const SCHEMA_VERSION: i64 = 4;
const SIGNAL_SCHEMA: &str = "signal_entry/v1";
const SOCIAL_CANDIDATE_SCHEMA: &str = "social_candidate/v1";
const SOCIAL_POST_SCHEMA: &str = "social_post/v1";
const SOCIAL_PUBLISH_RESERVATION_SCHEMA: &str = "social_publish_reservation/v1";
const UPSTREAM_IMPACT_SCHEMA: &str = "upstream_impact/v1";
const UPSTREAM_REVIEW_QUEUE_SCHEMA: &str = "upstream_review_queue/v1";
const UPSTREAM_REVIEW_SCHEMA: &str = "upstream_review/v1";
const CONFIG_FEATURE_CATALOG_SCHEMA: &str = "codex_config_feature_catalog/v1";
const ANALYSIS_DRAFT_KIND: &str = "analysis_draft";
const RADAR_ARCHIVE_HISTORICAL_RETENTION_CUTOFF: &str = "2026-06-07T00:00:00Z";
const UPSTREAM_REVIEW_LINEAR_FOLLOWUP_CUTOFF: &str = "2026-06-12T00:00:00Z";
const DEFAULT_VALIDATION_PATHS: &[&str] = &[
	".agent/automations/decodex/cache/github/bundles",
	".agent/automations/decodex/cache/github/review-queue",
	".agent/automations/decodex/cache/github/reviews",
	".agent/automations/decodex/cache/github/impact",
	".agent/automations/decodex/cache/github/control-plane-upgrades",
	".agent/automations/decodex/cache/github/social-candidates",
	".agent/automations/decodex/cache/social/x",
	".agent/automations/decodex/cache/site-content/signals",
	".agent/automations/decodex/cache/site-content/release-deltas",
	".agent/automations/decodex/cache/generated",
];
const RADAR_ARCHIVE_MANIFEST_SCHEMA: &str = "radar_archive_manifest/v1";
const ANALYSIS_MODES: &[&str] = &["commit_only", "pr_first"];
const SIGNAL_CONFIDENCE: &[&str] = &["confirmed", "likely", "weak"];
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
const UPSTREAM_SUBJECT_KINDS: &[&str] = &["commit", "pr"];
const GENERIC_COMMIT_TITLES: &[&str] =
	&["update", "fix", "fix.", "fix tests", "fix tests.", "merge fixes", "flaky syntax"];
const CONFIG_FEATURE_CATALOG_PATH: &str =
	".agent/automations/decodex/cache/generated/codex-config-features.json";
const RUN_CODEX_ANALYSIS_SCRIPT: &str = "automations/decodex/scripts/github/run_codex_analysis.py";
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
	&["archived", "control_plane", "deprecated", "seen", "signal", "skipped", "social", "watch"];
const ARTIFACT_KINDS: &[&str] = &[
	"analysis",
	"archive_manifest",
	"bundle",
	"control_plane_upgrade_candidate",
	"ledger_export",
	"release_delta",
	"signal",
	"social_candidate",
	"social_post",
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
struct ArtifactValidation {
	schema: Option<String>,
	errors: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default)]
struct ArtifactValidationOptions {
	allow_historical_archive_retention: bool,
	allow_historical_upstream_review_linear_followup: bool,
}

#[derive(Debug)]
struct ValidationState {
	active_social_publish_reservation_idempotency_keys: BTreeMap<String, PathBuf>,
	seen_terminal_social_post_idempotency_keys: BTreeMap<String, PathBuf>,
	seen_signal_slugs: BTreeMap<String, PathBuf>,
}
impl ValidationState {
	fn new() -> Self {
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

#[derive(Debug)]
struct PreparedReleaseDelta {
	path: PathBuf,
	cleanup_dir: Option<PathBuf>,
}
impl Drop for PreparedReleaseDelta {
	fn drop(&mut self) {
		if let Some(path) = &self.cleanup_dir {
			let _ = fs::remove_dir_all(path);
		}
	}
}

#[derive(Debug, Eq, PartialEq)]
struct ReleaseSelection {
	stable_tag: String,
	preview_tag: String,
	pr_numbers: Vec<u64>,
}

#[derive(Debug)]
struct BackfillPaths {
	bundle: PathBuf,
	analysis: PathBuf,
	signal: PathBuf,
}

struct RecentCommit {
	sha: String,
	title: String,
	url: String,
	committed_at: Option<String>,
}

#[derive(Clone, Debug)]
struct BundleFile {
	path: String,
	patch_excerpt: Option<String>,
}

#[derive(Clone, Debug)]
struct BundleCommit {
	sha: String,
	message: String,
}

#[derive(Clone, Debug)]
struct BundlePr {
	number: u64,
	title: String,
	body: String,
	state: String,
	url: String,
}

#[derive(Clone, Debug)]
struct SourceBundle {
	primary_pr: Option<BundlePr>,
	commits: Vec<BundleCommit>,
	files: Vec<BundleFile>,
}

#[derive(Debug)]
struct QueueBuild {
	queue: Value,
	ledger_enabled: bool,
}

#[derive(Clone, Debug)]
struct ReleasePair {
	stable: Value,
	preview: Value,
}

#[derive(Debug, Default)]
struct SocialPublishStateScan {
	published_count: usize,
	active_reservation_count: usize,
	idempotency_conflict: Option<PathBuf>,
}

#[derive(Debug)]
enum RefreshKind {
	Queue,
	ReleaseDelta,
}

pub(crate) fn refresh_queue(
	request: &RadarRefreshQueueRequest,
) -> crate::prelude::Result<RadarRefreshQueueReport> {
	let root = repo_root()?;
	let api = GitHubApi::new(github_token(request.token_env.as_deref()))?;
	let build = build_review_queue(request, &root, &api)?;
	let errors = validate_artifact_errors(&build.queue);

	if !errors.is_empty() {
		eyre::bail!("Upstream review queue validation failed:\n- {}", errors.join("\n- "));
	}
	if request.dry_run {
		println!("{}", pretty_json(&build.queue)?);

		return Ok(queue_report(
			&build.queue,
			false,
			build.ledger_enabled,
			&root,
			&request.queue_out,
		));
	}

	let out = absolute_repo_path(&root, &request.queue_out);
	let changed = write_json_if_material_changed(&out, &build.queue, RefreshKind::Queue)?;

	Ok(queue_report(&build.queue, changed, build.ledger_enabled, &root, &request.queue_out))
}

/// Refresh the stable-versus-prerelease release-delta artifact.
pub(crate) fn refresh_release_delta(
	request: &RadarRefreshReleaseDeltaRequest,
) -> crate::prelude::Result<RadarRefreshReleaseDeltaReport> {
	let root = repo_root()?;
	let api = GitHubApi::new(github_token(request.token_env.as_deref()))?;
	let payload = build_release_delta(request, &root, &api)?;
	let errors = validate_artifact_errors(&payload);

	if !errors.is_empty() {
		eyre::bail!("Release-delta validation failed:\n- {}", errors.join("\n- "));
	}
	if request.dry_run {
		println!("{}", pretty_json(&payload)?);

		return Ok(release_delta_report(&payload, false, &root, &request.out));
	}

	let out = absolute_repo_path(&root, &request.out);
	let changed = write_json_if_material_changed(&out, &payload, RefreshKind::ReleaseDelta)?;

	Ok(release_delta_report(&payload, changed, &root, &request.out))
}

/// Validate the requested Radar artifact paths.
pub(crate) fn validate(
	request: &RadarValidateRequest,
) -> crate::prelude::Result<RadarValidationReport> {
	let paths = validation_paths(&request.paths);
	let files = collect_json_files(&paths)?;
	let mut state = ValidationState::new();
	let mut errors = Vec::new();

	for path in &files {
		let payload = load_json(path)?;
		let validation = validate_artifact_for_path(path, &payload);

		if validation.schema.as_deref() == Some(SIGNAL_SCHEMA) {
			validate_signal_slug_uniqueness(path, &payload, &mut state, &mut errors);
		}
		if validation.schema.as_deref() == Some(SOCIAL_POST_SCHEMA) {
			validate_terminal_social_post_idempotency_key_uniqueness(
				path,
				&payload,
				&mut state,
				&mut errors,
			);
		}
		if validation.schema.as_deref() == Some(SOCIAL_PUBLISH_RESERVATION_SCHEMA) {
			validate_active_social_publish_reservation_uniqueness(
				path,
				&payload,
				&mut state,
				&mut errors,
			);
		}

		for error in validation.errors {
			errors.push(format!("{}: {error}", path.display()));
		}
	}

	if errors.is_empty() {
		Ok(RadarValidationReport { checked_files: files.len() })
	} else {
		Err(eyre::eyre!("Radar validation failed:\n- {}", errors.join("\n- ")))
	}
}

pub(crate) fn reserve_social_publish(
	request: &RadarSocialReservePublishRequest,
) -> crate::prelude::Result<RadarSocialReservePublishReport> {
	if request.slug.trim().is_empty() {
		return Err(eyre::eyre!("slug is required"));
	}
	if request.idempotency_key.trim().is_empty() {
		return Err(eyre::eyre!("idempotency_key is required"));
	}
	if request.daily_limit == 0 {
		return Err(eyre::eyre!("daily_limit must be positive"));
	}
	if request.candidate_paths.is_empty() && request.urls.is_empty() {
		return Err(eyre::eyre!("at least one candidate path or URL is required"));
	}
	if request.duplicate_keys.is_empty() {
		return Err(eyre::eyre!("at least one duplicate key is required"));
	}

	let root = repo_root()?;
	let out_dir = resolve_against(&root, &request.out_dir);
	let posts_dir = resolve_against(&root, &request.posts_dir);
	let reservation_path =
		out_dir.join(&request.day).join(format!("{}.json", slugify(&request.slug)));
	let scan =
		scan_social_publish_state(&out_dir, &posts_dir, &request.idempotency_key, &request.day)?;

	if scan.idempotency_conflict.is_some() {
		return Err(eyre::eyre!(
			"idempotency_key already has an active reservation or terminal post: {}",
			request.idempotency_key
		));
	}
	if scan.published_count + scan.active_reservation_count >= request.daily_limit {
		return Err(eyre::eyre!(
			"daily publish cap exhausted for {}: published={}, active_reservations={}, limit={}",
			request.day,
			scan.published_count,
			scan.active_reservation_count,
			request.daily_limit
		));
	}

	let payload = social_publish_reservation_payload(request, &root);
	let validation = validate_artifact(&payload);

	if !validation.errors.is_empty() {
		return Err(eyre::eyre!(
			"generated reservation failed validation: {}",
			validation.errors.join("; ")
		));
	}
	if !request.dry_run {
		write_new_json(&reservation_path, &payload)?;
	}

	Ok(RadarSocialReservePublishReport {
		status: if request.dry_run { "dry_run".into() } else { "reserved".into() },
		path: path_arg(&root, &reservation_path),
		idempotency_key: request.idempotency_key.clone(),
		daily_limit: request.daily_limit,
		published_count: scan.published_count,
		active_reservation_count: scan.active_reservation_count,
	})
}

/// Build a deterministic GitHub change bundle and write it to disk.
pub(crate) fn build_bundle(request: &RadarBundleBuildRequest) -> crate::prelude::Result<PathBuf> {
	let token = github_token(request.token_env.as_deref());
	let client = GithubClient::new(token.as_deref())?;
	let bundle = match (request.pr, request.commit.as_deref()) {
		(Some(pr_number), _) => client.build_pr_bundle(&request.repo, pr_number, &request.notes)?,
		(None, Some(commit_sha)) => {
			let promoted_pr = if request.force_commit_only {
				None
			} else {
				client.maybe_promote_commit_to_pr(&request.repo, commit_sha)
			};

			match promoted_pr {
				Some(pr_number) => {
					client.build_pr_bundle(&request.repo, pr_number, &request.notes)?
				},
				None => client.build_commit_bundle(&request.repo, commit_sha, &request.notes)?,
			}
		},
		(None, None) => eyre::bail!("one of --pr or --commit is required"),
	};

	write_json(&request.out, &bundle)?;

	Ok(request.out.clone())
}

/// Validate GitHub change bundle artifacts only.
pub(crate) fn validate_bundles(
	request: &RadarBundleValidateRequest,
) -> crate::prelude::Result<RadarValidationReport> {
	let files = collect_bundle_json_files(&request.paths)?;
	let mut errors = Vec::new();

	for path in &files {
		let payload = load_json(path)?;
		let validation = validate_artifact(&payload);

		if validation.schema.as_deref() != Some(BUNDLE_SCHEMA) {
			errors.push(format!("{}: schema must be {BUNDLE_SCHEMA}", path.display()));
		}

		for error in validation.errors {
			errors.push(format!("{}: {error}", path.display()));
		}
	}

	if errors.is_empty() {
		Ok(RadarValidationReport { checked_files: files.len() })
	} else {
		Err(eyre::eyre!("Bundle validation failed:\n- {}", errors.join("\n- ")))
	}
}

/// Render one `signal_entry/v1` artifact from a validated bundle and analysis draft.
pub(crate) fn render_signal(
	request: &RadarRenderSignalRequest,
) -> crate::prelude::Result<RadarRenderSignalReport> {
	let bundle = load_json(&request.bundle)?;
	let analysis = load_json(&request.analysis)?;

	validate_expected_schema(&bundle, BUNDLE_SCHEMA, "Bundle")?;
	validate_analysis_draft(&analysis)?;

	let root = repo_root()?;
	let known_features = load_known_feature_names(&root)?;
	let config_flags = rendered_config_flags(&bundle, &analysis, &known_features);
	let signal =
		rendered_signal(&bundle, &analysis, request.published_at.as_deref(), config_flags)?;

	validate_expected_schema(&signal, SIGNAL_SCHEMA, "Signal")?;
	write_json(&request.out, &signal)?;

	Ok(RadarRenderSignalReport { out: request.out.clone() })
}

/// Select and optionally execute release-window signal backfills.
pub(crate) fn backfill_release_range(
	request: &RadarBackfillReleaseRangeRequest,
) -> crate::prelude::Result<RadarBackfillReleaseRangeReport> {
	let root = repo_root()?;
	let prepared_release_delta = prepare_release_delta_path(request, &root)?;
	let release_delta = load_json(&prepared_release_delta.path)?;
	let selection = selected_release_comparison(
		&release_delta,
		request.stable_tag.as_deref(),
		request.preview_tag.as_deref(),
	)?;
	let signals_dir = resolve_against(&root, &request.signals_dir);
	let published = published_pr_numbers(&signals_dir)?;
	let mut target_prs = selection
		.pr_numbers
		.into_iter()
		.filter(|number| !published.contains(number))
		.collect::<Vec<_>>();

	if let Some(limit) = request.max_prs {
		target_prs.truncate(limit);
	}

	let mut report = RadarBackfillReleaseRangeReport {
		stable_tag: selection.stable_tag,
		preview_tag: selection.preview_tag,
		target_prs,
		created: 0,
		dry_run: request.dry_run,
	};

	if request.dry_run {
		return Ok(report);
	}

	for pr_number in &report.target_prs {
		let paths = signal_backfill_paths(&request.repo, *pr_number, request);
		let note = format!(
			"Backfilled from release compare range {}...{}",
			report.stable_tag, report.preview_tag
		);
		let bundle_path = resolve_against(&root, &paths.bundle);
		let analysis_path = resolve_against(&root, &paths.analysis);
		let signal_path = resolve_against(&root, &paths.signal);

		run_build_bundle(request, *pr_number, &bundle_path, &note)?;
		run_codex_analysis(&root, request, &bundle_path, &analysis_path)?;
		render_signal(&RadarRenderSignalRequest {
			bundle: bundle_path,
			analysis: analysis_path,
			out: signal_path,
			published_at: None,
		})?;

		report.created += 1;
	}

	validate(&RadarValidateRequest { paths: vec![resolve_against(&root, &request.signals_dir)] })?;
	run_refresh_release_delta(request, &request.release_delta, false)?;

	Ok(report)
}

fn validate_expected_schema(
	value: &Value,
	schema: &str,
	label: &str,
) -> crate::prelude::Result<()> {
	let validation = validate_artifact(value);

	if validation.schema.as_deref() != Some(schema) {
		return Err(eyre::eyre!("{label} schema must be {schema}"));
	}
	if !validation.errors.is_empty() {
		return Err(eyre::eyre!(
			"{label} validation failed:\n- {}",
			validation.errors.join("\n- ")
		));
	}

	Ok(())
}

fn build_review_queue(
	request: &RadarRefreshQueueRequest,
	root: &Path,
	api: &GitHubApi,
) -> crate::prelude::Result<QueueBuild> {
	let (default_branch, commits) = recent_commits(api, &request.repo, request.search_limit)?;
	let recent_commits_scanned = commits.len();
	let (published_prs, published_shas) =
		published_subjects(&absolute_repo_path(root, &request.signals_dir))?;
	let ledger_path = ledger_path(root, request);
	let mut ledger = ledger_path.as_deref().map(RadarLedger::open).transpose()?;
	let mut subjects = BTreeMap::<(String, String), Value>::new();
	let mut published_seen = 0_usize;

	for commit in commits {
		let pr_number = maybe_promote_commit_to_pr(api, &request.repo, &commit.sha)?;
		let subject_kind = if pr_number.is_some() { "pr" } else { "commit" };
		let subject_id = pr_number.map_or_else(|| commit.sha.clone(), |number| number.to_string());

		if let Some(ledger) = &mut ledger {
			ledger.record_commit(&request.repo, &commit, pr_number)?;
		}

		if published_shas.contains(&commit.sha)
			|| pr_number.is_some_and(|number| published_prs.contains(&number))
		{
			published_seen += 1;

			if let Some(ledger) = &mut ledger {
				ledger.record_review(
					&request.repo,
					subject_kind,
					&subject_id,
					"signal",
					"Already present in published signal collection.",
					Some("confirmed"),
				)?;
			}

			continue;
		}

		let key = (subject_kind.to_owned(), subject_id.clone());

		if let Some(current) = subjects.get_mut(&key) {
			append_commit_sha(current, &commit.sha);

			continue;
		}

		let bundle = match pr_number {
			Some(number) => build_pr_bundle(api, &request.repo, number)?,
			None => build_commit_bundle(api, &request.repo, &commit.sha)?,
		};

		subjects.insert(key, subject_from_bundle(&bundle, subject_kind, &subject_id, &commit));

		if let Some(ledger) = &mut ledger {
			ledger.record_review(
				&request.repo,
				subject_kind,
				&subject_id,
				"watch",
				"Queued for AI upstream review by deterministic Radar sync.",
				Some("likely"),
			)?;
		}
	}

	if let Some(ledger) = &mut ledger {
		ledger.commit()?;
	}

	let ordered_subjects = sort_queue_subjects(subjects.into_values().collect());
	let queue = review_queue_payload(
		request,
		&default_branch,
		recent_commits_scanned,
		published_seen,
		ordered_subjects,
	)?;

	Ok(QueueBuild { queue, ledger_enabled: !request.no_ledger })
}

fn build_release_delta(
	request: &RadarRefreshReleaseDeltaRequest,
	root: &Path,
	api: &GitHubApi,
) -> crate::prelude::Result<Value> {
	let releases = github_releases(api, &request.repo)?;
	let stable_release = select_release(&releases, &request.tag_prefix, false)?;
	let prerelease = select_release(&releases, &request.tag_prefix, true)?;
	let (stable_releases, preview_releases) = select_release_options(request, &releases)?;
	let release_pairs = select_release_pairs(
		request,
		root,
		&stable_release,
		&prerelease,
		&stable_releases,
		&preview_releases,
	)?;
	let signal_entries =
		load_signal_entries(&absolute_repo_path(root, &request.signals_dir), &request.repo)?;
	let mut comparison_entries = Vec::new();
	let mut default_tracked_signal_slugs = Vec::<String>::new();
	let mut default_compare_payload = None::<Value>;

	for pair in release_pairs {
		let is_default_pair = release_tag(&pair.stable) == release_tag(&stable_release)
			&& release_tag(&pair.preview) == release_tag(&prerelease);
		let comparison = build_release_comparison(api, request, &pair, &signal_entries)?;

		if is_default_pair {
			default_compare_payload = comparison.get("compare").cloned();
			default_tracked_signal_slugs = string_array_from_value(
				comparison.get("tracked_signal_slugs").unwrap_or(&Value::Null),
			);
		}

		comparison_entries.push(comparison);

		if request.pair_limit > 0
			&& comparison_entries.len() >= request.pair_limit
			&& default_compare_payload.is_some()
		{
			break;
		}
	}

	let Some(default_compare_payload) = default_compare_payload else {
		eyre::bail!("Default stable/prerelease pair was not included in comparison entries");
	};
	let (stable_options, preview_options) =
		filter_release_options(&stable_releases, &preview_releases, &comparison_entries);

	Ok(serde_json::json!({
		"schema": RELEASE_DELTA_SCHEMA,
		"repo": request.repo,
		"tag_prefix": request.tag_prefix,
		"generated_at": utc_now_iso()?,
		"stable_release": compact_release(&stable_release)?,
		"prerelease": compact_release(&prerelease)?,
		"compare": default_compare_payload,
		"release_options": {
			"stable": compact_releases(&stable_options)?,
			"preview": compact_releases(&preview_options)?,
		},
		"comparisons": comparison_entries,
		"tracked_signal_slugs": default_tracked_signal_slugs,
	}))
}

fn review_queue_payload(
	request: &RadarRefreshQueueRequest,
	default_branch: &str,
	recent_commits_scanned: usize,
	published_seen: usize,
	subjects: Vec<Value>,
) -> crate::prelude::Result<Value> {
	let critical = count_priority(&subjects, "critical");
	let high = count_priority(&subjects, "high");
	let normal = count_priority(&subjects, "normal");
	let low = count_priority(&subjects, "low");

	Ok(serde_json::json!({
		"schema": UPSTREAM_REVIEW_QUEUE_SCHEMA,
		"repo": request.repo,
		"generated_at": utc_now_iso()?,
		"source": {
			"default_branch": default_branch,
			"search_limit": request.search_limit,
			"signals_dir": request.signals_dir.to_string_lossy(),
		},
		"subjects": subjects,
		"counts": {
			"recent_commits_scanned": recent_commits_scanned,
			"published_subjects_seen": published_seen,
			"subjects_queued": critical + high + normal + low,
			"critical": critical,
			"high": high,
			"normal": normal,
			"low": low,
		},
	}))
}

fn count_priority(subjects: &[Value], priority: &str) -> usize {
	subjects
		.iter()
		.filter(|subject| {
			subject
				.get("review_priority")
				.and_then(Value::as_str)
				.is_some_and(|value| value == priority)
		})
		.count()
}

fn recent_commits(
	api: &GitHubApi,
	repo: &str,
	search_limit: usize,
) -> crate::prelude::Result<(String, Vec<RecentCommit>)> {
	let default_branch = repo_default_branch(api, repo)?;
	let url = format!(
		"https://api.github.com/repos/{repo}/commits?sha={}&per_page={search_limit}",
		percent_encode(&default_branch)
	);
	let payload = api.get(&url)?.payload;
	let Some(items) = payload.as_array() else {
		eyre::bail!("Expected commits list payload from GitHub API");
	};
	let commits = items.iter().filter_map(recent_commit_from_value).collect::<Vec<_>>();

	Ok((default_branch, commits))
}

fn recent_commit_from_value(item: &Value) -> Option<RecentCommit> {
	let commit = item.get("commit")?.as_object()?;
	let sha = item.get("sha")?.as_str()?.to_owned();
	let url = item.get("html_url")?.as_str()?.to_owned();
	let message = commit.get("message")?.as_str()?;

	if message.is_empty() {
		return None;
	}

	Some(RecentCommit {
		sha,
		title: first_line(message),
		url,
		committed_at: commit
			.get("committer")
			.and_then(Value::as_object)
			.and_then(|committer| committer.get("date"))
			.and_then(Value::as_str)
			.map(str::to_owned),
	})
}

fn published_subjects(
	signals_dir: &Path,
) -> crate::prelude::Result<(HashSet<u64>, HashSet<String>)> {
	let mut published_prs = HashSet::new();
	let mut published_shas = HashSet::new();

	for path in sorted_json_files(signals_dir)? {
		let payload = load_json(&path)?;

		validate_signal_file(&path, &payload)?;

		if let Some(pr_number) = payload
			.get("source_refs")
			.and_then(|refs| refs.get("pr_url"))
			.and_then(Value::as_str)
			.and_then(extract_pr_number_from_url)
		{
			published_prs.insert(pr_number);
		}

		for url in string_array(payload.pointer("/source_refs/commit_urls")) {
			if let Some(sha) = extract_commit_sha_from_url(&url) {
				published_shas.insert(sha);
			}
		}
	}

	Ok((published_prs, published_shas))
}

fn maybe_promote_commit_to_pr(
	api: &GitHubApi,
	repo: &str,
	commit_sha: &str,
) -> crate::prelude::Result<Option<u64>> {
	let url = format!("https://api.github.com/repos/{repo}/commits/{commit_sha}/pulls");
	let pulls = match api.get_paginated(&url) {
		Ok(pulls) => pulls,
		Err(_) => return Ok(None),
	};

	Ok(pulls.first().and_then(|first| first.get("number")).and_then(Value::as_u64))
}

fn build_pr_bundle(
	api: &GitHubApi,
	repo: &str,
	pr_number: u64,
) -> crate::prelude::Result<SourceBundle> {
	let pr = api.get(&format!("https://api.github.com/repos/{repo}/pulls/{pr_number}"))?.payload;
	let commits = api.get_paginated(&format!(
		"https://api.github.com/repos/{repo}/pulls/{pr_number}/commits?per_page=100"
	))?;
	let files = api.get_paginated(&format!(
		"https://api.github.com/repos/{repo}/pulls/{pr_number}/files?per_page=100"
	))?;

	Ok(SourceBundle {
		primary_pr: Some(BundlePr {
			number: required_value_u64(&pr, "number")?,
			title: required_value_string(&pr, "title")?,
			body: optional_value_string(&pr, "body").unwrap_or_default(),
			state: if optional_value_string(&pr, "merged_at").is_some() {
				"merged".to_owned()
			} else {
				required_value_string(&pr, "state")?
			},
			url: required_value_string(&pr, "html_url")?,
		}),
		commits: commits.iter().filter_map(bundle_commit_from_pr_commit).collect(),
		files: files.iter().filter_map(bundle_file_from_value).collect(),
	})
}

fn build_commit_bundle(
	api: &GitHubApi,
	repo: &str,
	commit_sha: &str,
) -> crate::prelude::Result<SourceBundle> {
	let commit =
		api.get(&format!("https://api.github.com/repos/{repo}/commits/{commit_sha}"))?.payload;
	let files = commit.get("files").and_then(Value::as_array).cloned().unwrap_or_default();
	let message = commit.pointer("/commit/message").and_then(Value::as_str).unwrap_or_default();

	Ok(SourceBundle {
		primary_pr: None,
		commits: vec![BundleCommit {
			sha: required_value_string(&commit, "sha")?,
			message: first_line(message),
		}],
		files: files.iter().filter_map(bundle_file_from_value).collect(),
	})
}

fn bundle_commit_from_pr_commit(item: &Value) -> Option<BundleCommit> {
	Some(BundleCommit {
		sha: item.get("sha")?.as_str()?.to_owned(),
		message: first_line(item.pointer("/commit/message")?.as_str()?),
	})
}

fn bundle_file_from_value(item: &Value) -> Option<BundleFile> {
	Some(BundleFile {
		path: item.get("filename")?.as_str()?.to_owned(),
		patch_excerpt: item.get("patch").and_then(Value::as_str).map(truncate_patch_excerpt),
	})
}

fn subject_from_bundle(
	bundle: &SourceBundle,
	subject_kind: &str,
	subject_id: &str,
	seed_commit: &RecentCommit,
) -> Value {
	let surface_hints = detect_surface_hints(bundle);
	let attention_flags = detect_attention_flags(bundle);
	let mut subject = serde_json::json!({
		"subject_kind": subject_kind,
		"subject_id": subject_id,
		"title": seed_commit.title.clone(),
		"url": seed_commit.url.clone(),
		"source_state": "commit_only",
		"commit_shas": commit_shas(bundle, seed_commit),
		"committed_at": seed_commit.committed_at.clone(),
		"changed_file_count": bundle.files.len(),
		"sample_paths": bundle.files.iter().take(12).map(|file| file.path.clone()).collect::<Vec<_>>(),
		"surface_hints": surface_hints,
		"attention_flags": attention_flags,
		"review_priority": priority_for(&surface_hints, &attention_flags),
		"review_reason": review_reason(&surface_hints, &attention_flags),
		"next_step": "ai_review_required",
	});

	if let Some(primary_pr) = &bundle.primary_pr
		&& let Some(subject) = subject.as_object_mut()
	{
		subject.insert("title".to_owned(), Value::String(primary_pr.title.clone()));
		subject.insert("url".to_owned(), Value::String(primary_pr.url.clone()));
		subject.insert("source_state".to_owned(), Value::String(primary_pr.state.clone()));
		subject.insert("pr_number".to_owned(), Value::from(primary_pr.number));
		subject.insert("pr_url".to_owned(), Value::String(primary_pr.url.clone()));
	}

	subject
}

fn commit_shas(bundle: &SourceBundle, seed_commit: &RecentCommit) -> Vec<String> {
	let shas = bundle.commits.iter().map(|commit| commit.sha.clone()).collect::<Vec<_>>();

	if shas.is_empty() { vec![seed_commit.sha.clone()] } else { shas }
}

fn append_commit_sha(subject: &mut Value, sha: &str) {
	let Some(shas) = subject.get_mut("commit_shas").and_then(Value::as_array_mut) else {
		return;
	};

	if !shas.iter().any(|value| value.as_str() == Some(sha)) {
		shas.push(Value::String(sha.to_owned()));
	}
}

fn sort_queue_subjects(mut subjects: Vec<Value>) -> Vec<Value> {
	subjects.sort_by_key(queue_sort_key);

	subjects
}

fn queue_sort_key(subject: &Value) -> (u8, String, String, String) {
	(
		match subject.get("review_priority").and_then(Value::as_str) {
			Some("critical") => 0,
			Some("high") => 1,
			Some("normal") => 2,
			Some("low") => 3,
			_ => 9,
		},
		subject.get("committed_at").and_then(Value::as_str).unwrap_or_default().to_owned(),
		subject.get("subject_kind").and_then(Value::as_str).unwrap_or_default().to_owned(),
		subject.get("subject_id").and_then(Value::as_str).unwrap_or_default().to_owned(),
	)
}

fn detect_surface_hints(bundle: &SourceBundle) -> Vec<String> {
	let haystack =
		bundle.files.iter().map(|file| file.path.to_lowercase()).collect::<Vec<_>>().join("\n");
	let mut hints = SURFACE_RULES
		.iter()
		.filter(|(_, terms)| terms.iter().any(|term| haystack.contains(term)))
		.map(|(surface, _)| (*surface).to_owned())
		.collect::<Vec<_>>();

	if hints.is_empty() {
		hints.push("internal_churn".to_owned());
	}

	hints.sort();

	hints
}

fn detect_attention_flags(bundle: &SourceBundle) -> Vec<String> {
	let haystack = text_blob(bundle);
	let mut flags = ATTENTION_RULES
		.iter()
		.filter(|(_, terms)| terms.iter().any(|term| haystack.contains(term)))
		.map(|(flag, _)| (*flag).to_owned())
		.collect::<Vec<_>>();

	flags.sort();

	flags
}

fn text_blob(bundle: &SourceBundle) -> String {
	let mut parts = Vec::new();

	if let Some(primary_pr) = &bundle.primary_pr {
		parts.push(primary_pr.title.clone());
		parts.push(primary_pr.body.clone());
	}

	parts.extend(bundle.commits.iter().map(|commit| commit.message.clone()));
	parts.extend(
		bundle
			.files
			.iter()
			.flat_map(|file| [file.path.clone(), file.patch_excerpt.clone().unwrap_or_default()]),
	);

	parts.join("\n").to_lowercase()
}

fn priority_for(surface_hints: &[String], attention_flags: &[String]) -> &'static str {
	let has_high_surface =
		surface_hints.iter().any(|surface| HIGH_VALUE_SURFACES.contains(&surface.as_str()));
	let breaking_or_removed = attention_flags
		.iter()
		.any(|flag| matches!(flag.as_str(), "breaking_change" | "deprecated_removed"));

	if breaking_or_removed && has_high_surface {
		"critical"
	} else if has_high_surface {
		"high"
	} else if attention_flags.iter().any(|flag| {
		matches!(flag.as_str(), "new_feature" | "protocol_change" | "release_packaging")
	}) {
		"normal"
	} else {
		"low"
	}
}

fn review_reason(surface_hints: &[String], attention_flags: &[String]) -> String {
	if surface_hints.iter().any(|hint| hint == "internal_churn") && attention_flags.is_empty() {
		return "Needs AI review because every recent upstream commit is tracked, but deterministic hints found only internal churn.".to_owned();
	}
	if !attention_flags.is_empty() {
		return format!("Needs AI review for {}.", attention_flags.join(", "));
	}

	format!("Needs AI review for surface hints: {}.", surface_hints.join(", "))
}

fn github_releases(api: &GitHubApi, repo: &str) -> crate::prelude::Result<Vec<Value>> {
	let mut releases = Vec::new();

	for page in 1..=5 {
		let payload = api
			.get(&format!("https://api.github.com/repos/{repo}/releases?per_page=100&page={page}"))?
			.payload;
		let Some(items) = payload.as_array() else {
			eyre::bail!("Expected releases list payload from GitHub API");
		};
		let count = items.len();

		releases.extend(items.iter().cloned());

		if count < 100 {
			break;
		}
	}

	Ok(releases)
}

fn select_release(
	releases: &[Value],
	tag_prefix: &str,
	prerelease: bool,
) -> crate::prelude::Result<Value> {
	releases
		.iter()
		.find(|release| {
			!release.get("draft").and_then(Value::as_bool).unwrap_or(false)
				&& release_tag(release).is_some_and(|tag| tag.starts_with(tag_prefix))
				&& release.get("prerelease").and_then(Value::as_bool).unwrap_or(false) == prerelease
		})
		.cloned()
		.ok_or_else(|| {
			let kind = if prerelease { "prerelease" } else { "stable release" };

			eyre::eyre!("No {kind} found for tag prefix {tag_prefix:?}")
		})
}

fn select_release_options(
	request: &RadarRefreshReleaseDeltaRequest,
	releases: &[Value],
) -> crate::prelude::Result<(Vec<Value>, Vec<Value>)> {
	let min_stable_key = stable_version_key(&request.min_stable_tag, &request.tag_prefix);
	let mut stable = relevant_releases(releases, &request.tag_prefix)
		.into_iter()
		.filter(|release| {
			!release.get("prerelease").and_then(Value::as_bool).unwrap_or(false)
				&& release_tag(release).is_some_and(|tag| {
					stable_version_key(tag, &request.tag_prefix) >= min_stable_key
				})
		})
		.collect::<Vec<_>>();
	let mut preview = relevant_releases(releases, &request.tag_prefix)
		.into_iter()
		.filter(|release| release.get("prerelease").and_then(Value::as_bool).unwrap_or(false))
		.collect::<Vec<_>>();

	if request.stable_limit > 0 {
		stable.truncate(request.stable_limit);
	}
	if request.preview_limit > 0 {
		preview.truncate(request.preview_limit);
	}
	if stable.is_empty() {
		eyre::bail!(
			"No stable releases found for tag prefix {:?} at or above {:?}",
			request.tag_prefix,
			request.min_stable_tag
		);
	}
	if preview.is_empty() {
		eyre::bail!("No prereleases found for tag prefix {:?}", request.tag_prefix);
	}

	Ok((stable, preview))
}

fn relevant_releases(releases: &[Value], tag_prefix: &str) -> Vec<Value> {
	releases
		.iter()
		.filter(|release| {
			!release.get("draft").and_then(Value::as_bool).unwrap_or(false)
				&& release_tag(release).is_some_and(|tag| tag.starts_with(tag_prefix))
		})
		.cloned()
		.collect()
}

fn select_release_pairs(
	request: &RadarRefreshReleaseDeltaRequest,
	root: &Path,
	stable_release: &Value,
	prerelease: &Value,
	stable_releases: &[Value],
	preview_releases: &[Value],
) -> crate::prelude::Result<Vec<ReleasePair>> {
	let default_pair = ReleasePair { stable: stable_release.clone(), preview: prerelease.clone() };
	let releases_by_tag = stable_releases
		.iter()
		.chain(preview_releases)
		.filter_map(|release| release_tag(release).map(|tag| (tag.to_owned(), release.clone())))
		.collect::<BTreeMap<_, _>>();
	let previous_pairs = previous_signal_pairs(&absolute_repo_path(root, &request.out))?
		.into_iter()
		.filter_map(|(stable_tag, preview_tag)| {
			Some(ReleasePair {
				stable: releases_by_tag.get(&stable_tag)?.clone(),
				preview: releases_by_tag.get(&preview_tag)?.clone(),
			})
		})
		.collect::<Vec<_>>();

	if previous_pairs.is_empty() {
		let mut pairs = vec![default_pair];

		pairs.extend(compare_candidates(stable_releases, preview_releases));

		let mut pairs = unique_release_pairs(pairs);

		if request.pair_limit > 0 {
			pairs.truncate(request.pair_limit);
		}

		Ok(pairs)
	} else {
		Ok(unique_release_pairs(iter::once(default_pair).chain(previous_pairs).collect()))
	}
}

fn compare_candidates(stable_releases: &[Value], preview_releases: &[Value]) -> Vec<ReleasePair> {
	let mut candidates = stable_releases
		.iter()
		.flat_map(|stable| {
			preview_releases
				.iter()
				.filter(move |preview| release_sort_key(preview) > release_sort_key(stable))
				.map(move |preview| ReleasePair {
					stable: stable.clone(),
					preview: preview.clone(),
				})
		})
		.collect::<Vec<_>>();

	candidates.sort_by(|left, right| {
		(release_sort_key(&right.preview), release_sort_key(&right.stable))
			.cmp(&(release_sort_key(&left.preview), release_sort_key(&left.stable)))
	});

	candidates
}

fn unique_release_pairs(pairs: Vec<ReleasePair>) -> Vec<ReleasePair> {
	let mut seen = BTreeSet::new();
	let mut unique = Vec::new();

	for pair in pairs {
		let Some(stable_tag) = release_tag(&pair.stable) else {
			continue;
		};
		let Some(preview_tag) = release_tag(&pair.preview) else {
			continue;
		};
		let key = (stable_tag.to_owned(), preview_tag.to_owned());

		if seen.insert(key) {
			unique.push(pair);
		}
	}

	unique
}

fn previous_signal_pairs(path: &Path) -> crate::prelude::Result<Vec<(String, String)>> {
	if !path.exists() {
		return Ok(Vec::new());
	}

	let Ok(previous) = load_json(path) else {
		return Ok(Vec::new());
	};
	let mut keys = Vec::new();
	let mut seen = BTreeSet::new();

	for comparison in previous.get("comparisons").and_then(Value::as_array).into_iter().flatten() {
		if string_array(comparison.get("tracked_signal_slugs")).is_empty() {
			continue;
		}

		let stable_tag = comparison.get("stable_tag_name").and_then(Value::as_str);
		let preview_tag = comparison.get("prerelease_tag_name").and_then(Value::as_str);
		let (Some(stable_tag), Some(preview_tag)) = (stable_tag, preview_tag) else {
			continue;
		};
		let key = (stable_tag.to_owned(), preview_tag.to_owned());

		if seen.insert(key.clone()) {
			keys.push(key);
		}
	}

	Ok(keys)
}

fn build_release_comparison(
	api: &GitHubApi,
	request: &RadarRefreshReleaseDeltaRequest,
	pair: &ReleasePair,
	signals: &[Value],
) -> crate::prelude::Result<Value> {
	let stable_tag = required_release_tag(&pair.stable)?;
	let preview_tag = required_release_tag(&pair.preview)?;
	let compare = api
		.get(&format!(
			"https://api.github.com/repos/{}/compare/{stable_tag}...{preview_tag}",
			request.repo
		))?
		.payload;
	let commits = compare
		.get("commits")
		.and_then(Value::as_array)
		.ok_or_else(|| eyre::eyre!("Expected compare.commits from GitHub API"))?;
	let commit_shas = commits
		.iter()
		.filter_map(|commit| commit.get("sha").and_then(Value::as_str).map(str::to_owned))
		.collect::<Vec<_>>();
	let pr_numbers = compare_pr_numbers(commits);
	let tracked_signal_slugs = tracked_signal_slugs(signals, &commit_shas, &pr_numbers);

	Ok(serde_json::json!({
		"stable_tag_name": stable_tag,
		"prerelease_tag_name": preview_tag,
		"compare": {
			"status": required_value_string(&compare, "status")?,
			"ahead_by": required_value_i64(&compare, "ahead_by")?,
			"total_commits": required_value_i64(&compare, "total_commits")?,
			"url": required_value_string(&compare, "html_url")?,
			"commit_shas": commit_shas,
			"pr_numbers": pr_numbers,
		},
		"tracked_signal_slugs": tracked_signal_slugs,
	}))
}

fn load_signal_entries(signals_dir: &Path, repo: &str) -> crate::prelude::Result<Vec<Value>> {
	let mut entries = Vec::new();

	for path in sorted_json_files(signals_dir)? {
		let payload = load_json(&path)?;

		validate_signal_file(&path, &payload)?;

		if payload.pointer("/source_refs/repo").and_then(Value::as_str) == Some(repo) {
			entries.push(payload);
		}
	}

	Ok(entries)
}

fn tracked_signal_slugs(
	signals: &[Value],
	commit_shas: &[String],
	pr_numbers: &[u64],
) -> Vec<String> {
	let commit_set = commit_shas.iter().map(String::as_str).collect::<HashSet<_>>();
	let pr_set = pr_numbers.iter().copied().collect::<HashSet<_>>();
	let mut sorted_signals = signals.iter().collect::<Vec<_>>();

	sorted_signals.sort_by(|left, right| {
		right
			.get("published_at")
			.and_then(Value::as_str)
			.unwrap_or_default()
			.cmp(left.get("published_at").and_then(Value::as_str).unwrap_or_default())
	});

	sorted_signals
		.into_iter()
		.filter(|signal| {
			let signal_shas = signal_commit_shas(signal);
			let signal_pr = signal_pr_number(signal);

			signal_shas.iter().any(|sha| commit_set.contains(sha.as_str()))
				|| signal_pr.is_some_and(|number| pr_set.contains(&number))
		})
		.filter_map(|signal| signal.get("slug").and_then(Value::as_str).map(str::to_owned))
		.collect()
}

fn signal_commit_shas(signal: &Value) -> Vec<String> {
	string_array(signal.pointer("/source_refs/commit_urls"))
		.into_iter()
		.filter_map(|url| extract_commit_sha_from_url(&url))
		.collect()
}

fn signal_pr_number(signal: &Value) -> Option<u64> {
	signal
		.pointer("/source_refs/pr_url")
		.and_then(Value::as_str)
		.and_then(extract_pr_number_from_url)
}

fn compare_pr_numbers(commits: &[Value]) -> Vec<u64> {
	let mut numbers = commits
		.iter()
		.flat_map(|commit| {
			commit
				.pointer("/commit/message")
				.and_then(Value::as_str)
				.map(pr_numbers_from_message)
				.unwrap_or_default()
		})
		.collect::<BTreeSet<_>>()
		.into_iter()
		.collect::<Vec<_>>();

	numbers.sort();

	numbers
}

fn pr_numbers_from_message(message: &str) -> Vec<u64> {
	let mut numbers = Vec::new();
	let mut rest = message;

	while let Some(start) = rest.find("(#") {
		let candidate = &rest[start + 2..];
		let Some(end) = candidate.find(')') else {
			break;
		};
		let digits = &candidate[..end];

		if !digits.is_empty()
			&& digits.chars().all(|ch| ch.is_ascii_digit())
			&& let Ok(number) = digits.parse::<u64>()
		{
			numbers.push(number);
		}

		rest = &candidate[end + 1..];
	}

	numbers
}

fn filter_release_options(
	stable_releases: &[Value],
	preview_releases: &[Value],
	comparison_entries: &[Value],
) -> (Vec<Value>, Vec<Value>) {
	let allowed_stable_tags = comparison_entries
		.iter()
		.filter_map(|entry| entry.get("stable_tag_name").and_then(Value::as_str))
		.collect::<BTreeSet<_>>();
	let allowed_preview_tags = comparison_entries
		.iter()
		.filter_map(|entry| entry.get("prerelease_tag_name").and_then(Value::as_str))
		.collect::<BTreeSet<_>>();
	let stable = stable_releases
		.iter()
		.filter(|release| release_tag(release).is_some_and(|tag| allowed_stable_tags.contains(tag)))
		.cloned()
		.collect();
	let preview = preview_releases
		.iter()
		.filter(|release| {
			release_tag(release).is_some_and(|tag| allowed_preview_tags.contains(tag))
		})
		.cloned()
		.collect();

	(stable, preview)
}

fn compact_releases(releases: &[Value]) -> crate::prelude::Result<Vec<Value>> {
	releases.iter().map(compact_release).collect()
}

fn compact_release(release: &Value) -> crate::prelude::Result<Value> {
	let tag_name = required_release_tag(release)?;

	Ok(serde_json::json!({
		"tag_name": tag_name,
		"name": optional_value_string(release, "name").unwrap_or_else(|| tag_name.to_owned()),
		"prerelease": release.get("prerelease").and_then(Value::as_bool).unwrap_or(false),
		"published_at": required_value_string(release, "published_at")?,
		"url": required_value_string(release, "html_url")?,
	}))
}

fn stable_version_key(tag_name: &str, tag_prefix: &str) -> Vec<u64> {
	tag_name
		.strip_prefix(tag_prefix)
		.unwrap_or(tag_name)
		.split('.')
		.map(|part| {
			let digits = part.chars().filter(|ch| ch.is_ascii_digit()).collect::<String>();

			digits.parse::<u64>().unwrap_or(0)
		})
		.collect()
}

fn release_sort_key(release: &Value) -> &str {
	release.get("published_at").and_then(Value::as_str).unwrap_or_default()
}

fn required_release_tag(release: &Value) -> crate::prelude::Result<&str> {
	release_tag(release).ok_or_else(|| eyre::eyre!("Release payload is missing tag_name"))
}

fn release_tag(release: &Value) -> Option<&str> {
	release.get("tag_name").and_then(Value::as_str)
}

fn repo_default_branch(api: &GitHubApi, repo: &str) -> crate::prelude::Result<String> {
	let payload = api.get(&format!("https://api.github.com/repos/{repo}"))?.payload;

	required_value_string(&payload, "default_branch")
		.map_err(|error| eyre::eyre!("Unable to resolve default branch for {repo}: {error}"))
}

fn absolute_repo_path(root: &Path, path: &Path) -> PathBuf {
	if path.is_absolute() { path.to_path_buf() } else { root.join(path) }
}

fn ledger_path(root: &Path, request: &RadarRefreshQueueRequest) -> Option<PathBuf> {
	(!request.no_ledger).then(|| absolute_repo_path(root, &request.ledger))
}

fn sorted_json_files(path: &Path) -> crate::prelude::Result<Vec<PathBuf>> {
	if !path.exists() {
		return Ok(Vec::new());
	}

	let mut files = fs::read_dir(path)?
		.map(|entry| entry.map(|entry| entry.path()))
		.collect::<std::result::Result<Vec<_>, _>>()?;

	files.retain(|path| {
		path.is_file() && path.extension().is_some_and(|extension| extension == "json")
	});
	files.sort();

	Ok(files)
}

fn collect_bundle_json_files(paths: &[PathBuf]) -> crate::prelude::Result<Vec<PathBuf>> {
	if paths.is_empty() {
		eyre::bail!("at least one bundle JSON file or directory is required");
	}

	let mut files = Vec::new();

	for path in paths {
		if path.is_dir() {
			files.extend(sorted_json_files(path)?);
		} else if path.is_file() {
			files.push(path.clone());
		} else {
			eyre::bail!("Bundle validation path does not exist: {}", path.display());
		}
	}

	files.sort();

	Ok(files)
}

fn validate_signal_file(path: &Path, payload: &Value) -> crate::prelude::Result<()> {
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

fn validate_analysis_draft(value: &Value) -> crate::prelude::Result<()> {
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

fn validate_artifact_errors(payload: &Value) -> Vec<String> {
	validate_artifact(payload).errors
}

fn write_json_if_material_changed(
	path: &Path,
	payload: &Value,
	kind: RefreshKind,
) -> crate::prelude::Result<bool> {
	if let Ok(existing) = load_json(path)
		&& material_json(&existing, &kind) == material_json(payload, &kind)
	{
		return Ok(false);
	}
	if let Some(parent) = path.parent() {
		fs::create_dir_all(parent)?;
	}

	fs::write(path, format!("{}\n", pretty_json(payload)?))?;

	Ok(true)
}

fn material_json(payload: &Value, kind: &RefreshKind) -> Value {
	let mut normalized = payload.clone();

	match kind {
		RefreshKind::Queue | RefreshKind::ReleaseDelta => {
			if let Some(object) = normalized.as_object_mut() {
				object.insert("generated_at".to_owned(), Value::String(String::new()));
			}
		},
	}

	normalized
}

fn load_known_feature_names(root: &Path) -> crate::prelude::Result<BTreeSet<String>> {
	let path = root.join(CONFIG_FEATURE_CATALOG_PATH);

	if !path.exists() {
		return Ok(BTreeSet::new());
	}

	let payload = load_json(&path)?;
	let names = payload
		.get("features")
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
		.filter_map(|item| item.get("name").and_then(Value::as_str))
		.filter(|name| !name.is_empty())
		.map(str::to_owned)
		.collect();

	Ok(names)
}

fn short_sha(value: &str) -> String {
	value.chars().take(7).collect()
}

fn slugify(value: &str) -> String {
	let mut slug = String::new();
	let mut previous_was_separator = false;

	for character in value.chars().flat_map(char::to_lowercase) {
		if character.is_ascii_lowercase() || character.is_ascii_digit() {
			slug.push(character);

			previous_was_separator = false;
		} else if !previous_was_separator && !slug.is_empty() {
			slug.push('-');

			previous_was_separator = true;
		}
	}

	while slug.ends_with('-') {
		slug.pop();
	}

	if slug.is_empty() { "signal".into() } else { slug }
}

fn repo_root() -> crate::prelude::Result<PathBuf> {
	let mut candidate = env::current_dir()?;

	loop {
		if candidate.join("automations/decodex/scripts/github/README.md").is_file()
			&& candidate.join("apps/decodex/src/radar.rs").is_file()
		{
			return Ok(candidate);
		}
		if !candidate.pop() {
			return Err(eyre::eyre!(
				"Unable to find Decodex repository root from current directory"
			));
		}
	}
}

fn selected_release_comparison(
	payload: &Value,
	stable_tag: Option<&str>,
	preview_tag: Option<&str>,
) -> crate::prelude::Result<ReleaseSelection> {
	validate_expected_schema(payload, RELEASE_DELTA_SCHEMA, "Release-delta")?;

	let entry =
		payload.as_object().ok_or_else(|| eyre::eyre!("Release-delta must be an object"))?;
	let target_stable = stable_tag
		.map(str::to_owned)
		.or_else(|| release_delta_release_tag(entry.get("stable_release")))
		.ok_or_else(|| eyre::eyre!("stable release tag could not be selected"))?;
	let target_preview = preview_tag
		.map(str::to_owned)
		.or_else(|| release_delta_release_tag(entry.get("prerelease")))
		.ok_or_else(|| eyre::eyre!("preview release tag could not be selected"))?;
	let comparisons = entry
		.get("comparisons")
		.and_then(Value::as_array)
		.ok_or_else(|| eyre::eyre!("Release-delta comparisons must be a list"))?;

	for comparison in comparisons {
		let Some(comparison) = comparison.as_object() else {
			continue;
		};

		if string_field(comparison, "stable_tag_name") == Some(target_stable.as_str())
			&& string_field(comparison, "prerelease_tag_name") == Some(target_preview.as_str())
		{
			return Ok(ReleaseSelection {
				stable_tag: target_stable,
				preview_tag: target_preview,
				pr_numbers: comparison_pr_numbers(comparison),
			});
		}
	}

	Err(eyre::eyre!("No comparison found for {target_stable} -> {target_preview}"))
}

fn release_delta_release_tag(value: Option<&Value>) -> Option<String> {
	value
		.and_then(Value::as_object)
		.and_then(|release| string_field(release, "tag_name"))
		.filter(|tag| !tag.is_empty())
		.map(str::to_owned)
}

fn comparison_pr_numbers(comparison: &Map<String, Value>) -> Vec<u64> {
	comparison
		.get("compare")
		.and_then(Value::as_object)
		.and_then(|compare| compare.get("pr_numbers"))
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
		.filter_map(Value::as_u64)
		.collect()
}

fn published_pr_numbers(signals_dir: &Path) -> crate::prelude::Result<BTreeSet<u64>> {
	let mut published = BTreeSet::new();
	let mut files = Vec::new();

	for entry in fs::read_dir(signals_dir)? {
		let path = entry?.path();

		if path.extension().is_some_and(|extension| extension == "json") {
			files.push(path);
		}
	}

	files.sort();

	for path in files {
		let payload = load_json(&path)?;

		validate_expected_schema(&payload, SIGNAL_SCHEMA, "Signal")?;

		if let Some(pr_number) = payload
			.get("source_refs")
			.and_then(Value::as_object)
			.and_then(|refs| string_field(refs, "pr_url"))
			.and_then(pr_number_from_url)
		{
			published.insert(pr_number);
		}
	}

	Ok(published)
}

fn pr_number_from_url(value: &str) -> Option<u64> {
	let marker = "/pull/";
	let index = value.rfind(marker)?;
	let number = &value[index + marker.len()..];

	(!number.is_empty() && number.chars().all(|character| character.is_ascii_digit()))
		.then(|| number.parse().ok())
		.flatten()
}

fn prepare_release_delta_path(
	request: &RadarBackfillReleaseRangeRequest,
	root: &Path,
) -> crate::prelude::Result<PreparedReleaseDelta> {
	if !request.refresh_release_delta_first {
		return Ok(PreparedReleaseDelta {
			path: resolve_against(root, &request.release_delta),
			cleanup_dir: None,
		});
	}

	let temp_root = env::temp_dir().join(format!(
		"decodex-prerelease-delta-{}-{}",
		process::id(),
		OffsetDateTime::now_utc().unix_timestamp_nanos()
	));

	fs::create_dir_all(&temp_root)?;

	let release_delta = temp_root.join("release-delta.json");

	run_refresh_release_delta(request, &release_delta, true)?;

	Ok(PreparedReleaseDelta { path: release_delta, cleanup_dir: Some(temp_root) })
}

fn run_build_bundle(
	request: &RadarBackfillReleaseRangeRequest,
	pr_number: u64,
	out: &Path,
	note: &str,
) -> crate::prelude::Result<()> {
	build_bundle(&RadarBundleBuildRequest {
		repo: request.repo.clone(),
		pr: Some(pr_number),
		commit: None,
		force_commit_only: false,
		token_env: request.token_env.clone(),
		out: out.to_path_buf(),
		notes: vec![note.to_owned()],
	})?;

	Ok(())
}

fn run_codex_analysis(
	root: &Path,
	request: &RadarBackfillReleaseRangeRequest,
	bundle: &Path,
	out: &Path,
) -> crate::prelude::Result<()> {
	let mut command = helper_command(root, request, RUN_CODEX_ANALYSIS_SCRIPT);

	command.arg("--allow-ai-analysis-boundary");
	command.args([
		"--bundle",
		&path_arg(root, bundle),
		"--out",
		&path_arg(root, out),
		"--repo-root",
		&root.display().to_string(),
		"--codex-bin",
		request.codex_bin.as_str(),
	]);

	if let Some(model) = &request.model {
		command.args(["--model", model]);
	}

	run_helper(command, RUN_CODEX_ANALYSIS_SCRIPT)
}

fn run_refresh_release_delta(
	request: &RadarBackfillReleaseRangeRequest,
	out: &Path,
	include_refresh_limits: bool,
) -> crate::prelude::Result<()> {
	let mut refresh_request = RadarRefreshReleaseDeltaRequest {
		repo: request.repo.clone(),
		signals_dir: request.signals_dir.clone(),
		out: out.to_path_buf(),
		token_env: request.token_env.clone(),
		..RadarRefreshReleaseDeltaRequest::default()
	};

	if include_refresh_limits {
		if let Some(limit) = request.refresh_stable_limit {
			refresh_request.stable_limit = limit;
		}
		if let Some(limit) = request.refresh_preview_limit {
			refresh_request.preview_limit = limit;
		}
		if let Some(limit) = request.refresh_pair_limit {
			refresh_request.pair_limit = limit;
		}
	}

	refresh_release_delta(&refresh_request)?;

	Ok(())
}

fn helper_command(
	root: &Path,
	request: &RadarBackfillReleaseRangeRequest,
	script: &str,
) -> Command {
	let mut command = Command::new(&request.python_bin);

	command.current_dir(root).arg(root.join(script));

	command
}

fn run_helper(mut command: Command, script: &str) -> crate::prelude::Result<()> {
	let output = command.output()?;

	if output.status.success() {
		return Ok(());
	}

	let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
	let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
	let details = if !stderr.is_empty() {
		stderr
	} else if !stdout.is_empty() {
		stdout
	} else {
		"unknown error".into()
	};

	Err(eyre::eyre!("{script} failed: {details}"))
}

fn signal_backfill_paths(
	repo: &str,
	pr_number: u64,
	request: &RadarBackfillReleaseRangeRequest,
) -> BackfillPaths {
	let stem = format!("{}-pr-{pr_number}", repo_path_stem(repo));

	BackfillPaths {
		bundle: request.bundles_dir.join(format!("{stem}.json")),
		analysis: request.analysis_dir.join(format!("{stem}.analysis.json")),
		signal: request.signals_dir.join(format!("{stem}.json")),
	}
}

fn repo_path_stem(repo: &str) -> String {
	repo.chars()
		.map(
			|character| {
				if character.is_ascii_alphanumeric() { character.to_ascii_lowercase() } else { '-' }
			},
		)
		.collect::<String>()
		.trim_matches('-')
		.to_owned()
}

fn resolve_against(root: &Path, path: &Path) -> PathBuf {
	if path.is_absolute() { path.to_path_buf() } else { root.join(path) }
}

fn path_arg(root: &Path, path: &Path) -> String {
	path.strip_prefix(root).unwrap_or(path).display().to_string()
}

fn pretty_json(payload: &Value) -> crate::prelude::Result<String> {
	serde_json::to_string_pretty(payload).map_err(Into::into)
}

fn body_excerpt(body: &str) -> String {
	let compact = body.split_whitespace().collect::<Vec<_>>().join(" ");

	if compact.chars().count() > 500 {
		format!("{}...", compact.chars().take(500).collect::<String>())
	} else {
		compact
	}
}

fn required_value_string(payload: &Value, field: &str) -> crate::prelude::Result<String> {
	payload
		.get(field)
		.and_then(Value::as_str)
		.filter(|value| !value.is_empty())
		.map(str::to_owned)
		.ok_or_else(|| eyre::eyre!("{field} must be a non-empty string"))
}

fn optional_value_string(payload: &Value, field: &str) -> Option<String> {
	payload.get(field).and_then(Value::as_str).filter(|value| !value.is_empty()).map(str::to_owned)
}

fn required_value_u64(payload: &Value, field: &str) -> crate::prelude::Result<u64> {
	payload
		.get(field)
		.and_then(Value::as_u64)
		.ok_or_else(|| eyre::eyre!("{field} must be a positive integer"))
}

fn required_value_i64(payload: &Value, field: &str) -> crate::prelude::Result<i64> {
	payload
		.get(field)
		.and_then(Value::as_i64)
		.ok_or_else(|| eyre::eyre!("{field} must be an integer"))
}

fn truncate_patch_excerpt(value: &str) -> String {
	let compact = value.trim();

	if compact.chars().count() > 900 {
		format!("{}...", compact.chars().take(900).collect::<String>())
	} else {
		compact.to_owned()
	}
}

fn string_array(value: Option<&Value>) -> Vec<String> {
	value
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
		.filter_map(|item| item.as_str().map(str::to_owned))
		.collect()
}

fn string_array_from_value(value: &Value) -> Vec<String> {
	string_array(Some(value))
}

fn extract_commit_sha_from_url(url: &str) -> Option<String> {
	let sha = url.rsplit_once("/commit/")?.1;

	(sha.len() >= 7 && sha.len() <= 40 && sha.chars().all(|ch| ch.is_ascii_hexdigit()))
		.then(|| sha.to_owned())
}

fn extract_pr_number_from_url(url: &str) -> Option<u64> {
	let number = url.rsplit_once("/pull/")?.1;

	(!number.is_empty() && number.chars().all(|ch| ch.is_ascii_digit()))
		.then(|| number.parse::<u64>().ok())
		.flatten()
}

fn percent_encode(value: &str) -> String {
	let mut encoded = String::new();

	for byte in value.bytes() {
		if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
			encoded.push(char::from(byte));
		} else {
			encoded.push_str(&format!("%{byte:02X}"));
		}
	}

	encoded
}

fn queue_report(
	queue: &Value,
	changed: bool,
	ledger_enabled: bool,
	root: &Path,
	queue_out: &Path,
) -> RadarRefreshQueueReport {
	let counts = queue.get("counts").and_then(Value::as_object);

	RadarRefreshQueueReport {
		changed,
		recent_commits_scanned: count_field(counts, "recent_commits_scanned"),
		published_subjects_seen: count_field(counts, "published_subjects_seen"),
		subjects_queued: count_field(counts, "subjects_queued"),
		ledger_enabled,
		queue_out: absolute_repo_path(root, queue_out),
	}
}

fn release_delta_report(
	payload: &Value,
	changed: bool,
	root: &Path,
	out: &Path,
) -> RadarRefreshReleaseDeltaReport {
	RadarRefreshReleaseDeltaReport {
		changed,
		stable_tag_name: payload
			.pointer("/stable_release/tag_name")
			.and_then(Value::as_str)
			.unwrap_or_default()
			.to_owned(),
		prerelease_tag_name: payload
			.pointer("/prerelease/tag_name")
			.and_then(Value::as_str)
			.unwrap_or_default()
			.to_owned(),
		comparisons: payload.get("comparisons").and_then(Value::as_array).map_or(0, Vec::len),
		out: absolute_repo_path(root, out),
	}
}

fn count_field(counts: Option<&Map<String, Value>>, field: &str) -> usize {
	counts
		.and_then(|counts| counts.get(field))
		.and_then(Value::as_u64)
		.and_then(|value| usize::try_from(value).ok())
		.unwrap_or_default()
}

fn validation_paths(paths: &[PathBuf]) -> Vec<PathBuf> {
	if paths.is_empty() {
		DEFAULT_VALIDATION_PATHS.iter().map(PathBuf::from).collect()
	} else {
		paths.to_vec()
	}
}

fn collect_json_files(paths: &[PathBuf]) -> crate::prelude::Result<Vec<PathBuf>> {
	let mut files = Vec::new();

	for path in paths {
		collect_json_path(path, &mut files)?;
	}

	files.sort();

	Ok(files)
}

fn collect_json_path(path: &Path, files: &mut Vec<PathBuf>) -> crate::prelude::Result<()> {
	if path.is_dir() {
		let mut children = fs::read_dir(path)?
			.map(|entry| entry.map(|entry| entry.path()))
			.collect::<std::result::Result<Vec<_>, _>>()?;

		children.sort();

		for child in children {
			collect_json_path(&child, files)?;
		}
	} else if path.is_file() {
		if path.extension().is_some_and(|extension| extension == "json") {
			files.push(path.to_path_buf());
		}
	} else {
		return Err(eyre::eyre!("Radar validation path does not exist: {}", path.display()));
	}

	Ok(())
}

fn load_json(path: &Path) -> crate::prelude::Result<Value> {
	let raw = fs::read_to_string(path)?;

	serde_json::from_str(&raw)
		.map_err(|error| eyre::eyre!("Failed to parse JSON from {}: {error}", path.display()))
}

fn write_json(path: &Path, payload: &Value) -> crate::prelude::Result<()> {
	if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
		fs::create_dir_all(parent)?;
	}

	let mut output = serde_json::to_string_pretty(payload)?;

	output.push('\n');

	let parent = path.parent().unwrap_or_else(|| Path::new("."));
	let file_name = path
		.file_name()
		.and_then(|name| name.to_str())
		.ok_or_else(|| eyre::eyre!("JSON output path must end in a valid file name"))?;
	let temp_path = parent.join(format!(".{file_name}.tmp-{}", process::id()));
	let write_result = (|| -> crate::prelude::Result<()> {
		let mut file = OpenOptions::new().write(true).create_new(true).open(&temp_path)?;

		file.write_all(output.as_bytes())?;
		file.sync_all()?;

		fs::rename(&temp_path, path)?;

		Ok(())
	})();

	if write_result.is_err() {
		let _ = fs::remove_file(&temp_path);
	}

	write_result?;

	Ok(())
}

fn write_new_json(path: &Path, payload: &Value) -> crate::prelude::Result<()> {
	if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
		fs::create_dir_all(parent)?;
	}

	let mut output = serde_json::to_string_pretty(payload)?;

	output.push('\n');

	let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;

	file.write_all(output.as_bytes())?;
	file.sync_all()?;

	Ok(())
}

fn scan_social_publish_state(
	reservations_dir: &Path,
	posts_dir: &Path,
	idempotency_key: &str,
	day: &str,
) -> crate::prelude::Result<SocialPublishStateScan> {
	let mut scan = SocialPublishStateScan::default();

	for payload_path in existing_json_files(reservations_dir)? {
		let payload = load_json(&payload_path)?;

		if payload.get("schema").and_then(Value::as_str) != Some(SOCIAL_PUBLISH_RESERVATION_SCHEMA)
		{
			continue;
		}
		if payload.get("status").and_then(Value::as_str) == Some("active") {
			if payload.get("day").and_then(Value::as_str) == Some(day) {
				scan.active_reservation_count += 1;
			}
			if payload.get("idempotency_key").and_then(Value::as_str) == Some(idempotency_key) {
				scan.idempotency_conflict.get_or_insert(payload_path);
			}
		}
	}
	for payload_path in existing_json_files(posts_dir)? {
		let payload = load_json(&payload_path)?;

		if payload.get("schema").and_then(Value::as_str) != Some(SOCIAL_POST_SCHEMA) {
			continue;
		}

		let status = payload.get("status").and_then(Value::as_str);

		if status == Some("published")
			&& payload
				.get("decision")
				.and_then(Value::as_object)
				.and_then(|decision| decision.get("day"))
				.and_then(Value::as_str)
				== Some(day)
		{
			scan.published_count += 1;
		}
		if status != Some("failed")
			&& payload
				.get("decision")
				.and_then(Value::as_object)
				.and_then(|decision| decision.get("idempotency_key"))
				.and_then(Value::as_str)
				== Some(idempotency_key)
		{
			scan.idempotency_conflict.get_or_insert(payload_path);
		}
	}

	Ok(scan)
}

fn existing_json_files(path: &Path) -> crate::prelude::Result<Vec<PathBuf>> {
	if !path.exists() {
		return Ok(Vec::new());
	}

	collect_json_files(&[path.to_path_buf()])
}

fn social_publish_reservation_payload(
	request: &RadarSocialReservePublishRequest,
	root: &Path,
) -> Value {
	let mut refs = Map::new();

	if !request.candidate_paths.is_empty() {
		refs.insert(
			"social_candidates".into(),
			Value::Array(
				request
					.candidate_paths
					.iter()
					.map(|path| Value::String(path_arg(root, &resolve_against(root, path))))
					.collect(),
			),
		);
	}
	if !request.urls.is_empty() {
		refs.insert(
			"urls".into(),
			Value::Array(request.urls.iter().cloned().map(Value::String).collect()),
		);
	}

	let mut owner = Map::new();

	if let Some(value) = request.automation_id.as_deref().filter(|value| !value.is_empty()) {
		owner.insert("automation_id".into(), Value::String(value.to_owned()));
	}
	if let Some(value) = request.run_id.as_deref().filter(|value| !value.is_empty()) {
		owner.insert("run_id".into(), Value::String(value.to_owned()));
	}
	if let Some(value) = request.branch.as_deref().filter(|value| !value.is_empty()) {
		owner.insert("branch".into(), Value::String(value.to_owned()));
	}

	let mut payload = serde_json::json!({
		"schema": SOCIAL_PUBLISH_RESERVATION_SCHEMA,
		"slug": request.slug,
		"channel": "x",
		"target_account": "decodexspace",
		"controller_account": "hackink",
		"mode": request.mode,
		"status": "active",
		"idempotency_key": request.idempotency_key,
		"reserved_at": request.reserved_at,
		"expires_at": request.expires_at,
		"day": request.day,
		"timezone": request.timezone,
		"candidate_refs": refs,
		"duplicate_keys": request.duplicate_keys,
	});

	if !owner.is_empty() {
		payload["owner"] = Value::Object(owner);
	}

	payload
}

fn require_member(value: &str, allowed: &[&str], label: &str) -> crate::prelude::Result<()> {
	if allowed.contains(&value) {
		Ok(())
	} else {
		eyre::bail!("{label} must be one of {}", choices(allowed))
	}
}

fn utc_now_iso() -> crate::prelude::Result<String> {
	Ok(OffsetDateTime::now_utc().format(&Rfc3339)?)
}

fn object_value<'a>(
	value: &'a Value,
	label: &str,
) -> crate::prelude::Result<&'a Map<String, Value>> {
	value.as_object().ok_or_else(|| eyre::eyre!("{label} must be an object"))
}

fn required_string<'a>(
	object: &'a Map<String, Value>,
	field: &str,
	label: &str,
) -> crate::prelude::Result<&'a str> {
	string_field(object, field)
		.filter(|value| !value.is_empty())
		.ok_or_else(|| eyre::eyre!("{label} must be a non-empty string"))
}

fn optional_string<'a>(object: &'a Map<String, Value>, field: &str) -> Option<&'a str> {
	object.get(field).and_then(Value::as_str)
}

fn build_pr_bundle_from_sources(
	repo: &str,
	pr: &Value,
	commits: &[Value],
	files: &[Value],
	default_branch: &str,
	notes: &[String],
) -> crate::prelude::Result<Value> {
	let pr = object_value(pr, "pull request")?;
	let commit_items =
		commits.iter().map(commit_bundle_item).collect::<crate::prelude::Result<Vec<_>>>()?;
	let file_items =
		files.iter().map(file_bundle_item).collect::<crate::prelude::Result<Vec<_>>>()?;
	let docs_refs = collect_docs_refs(files);
	let examples_refs = collect_examples_refs(files);
	let all_patch_text = files
		.iter()
		.filter_map(|file| file.get("patch").and_then(Value::as_str))
		.collect::<Vec<_>>()
		.join("\n");
	let all_commit_text = commits
		.iter()
		.filter_map(|commit| {
			commit
				.get("commit")
				.and_then(Value::as_object)
				.and_then(|commit| commit.get("message"))
				.and_then(Value::as_str)
		})
		.collect::<Vec<_>>()
		.join("\n");
	let mut bundle_notes =
		vec!["Built from GitHub pull-request, commits, files, and repo endpoints.".to_owned()];

	bundle_notes.extend(notes.iter().cloned());

	let primary_pr = serde_json::json!({
		"number": required_u64(pr, "number", "primary_pr.number")?,
		"title": required_string(pr, "title", "primary_pr.title")?,
		"body": pr.get("body").and_then(Value::as_str).unwrap_or(""),
		"state": pr
			.get("merged_at")
			.and_then(Value::as_str)
			.filter(|value| !value.is_empty())
			.map_or_else(
				|| required_string(pr, "state", "primary_pr.state").map(str::to_owned),
				|_| Ok("merged".to_owned()),
			)?,
		"merged_at": pr.get("merged_at").cloned().unwrap_or(Value::Null),
		"labels": pr_labels(pr),
		"url": required_string(pr, "html_url", "primary_pr.url")?,
	});
	let bundle = serde_json::json!({
		"schema": BUNDLE_SCHEMA,
		"repo": repo,
		"analysis_mode": "pr_first",
		"default_branch": default_branch,
		"primary_pr": primary_pr,
		"commits": commit_items,
		"files": file_items,
		"linked_issues": collect_issue_refs(
			&[pr.get("body").and_then(Value::as_str).unwrap_or(""), &all_commit_text]
		)?,
		"extracted_flags": collect_flags(&[
			pr.get("body").and_then(Value::as_str).unwrap_or(""),
			&all_commit_text,
			&all_patch_text,
		])?,
		"docs_refs": docs_refs,
		"examples_refs": examples_refs,
		"notes": bundle_notes,
	});

	validate_bundle_value(&bundle)?;

	Ok(bundle)
}

fn build_commit_bundle_from_sources(
	repo: &str,
	commit: &Value,
	default_branch: &str,
	notes: &[String],
) -> crate::prelude::Result<Value> {
	let commit = object_value(commit, "commit")?;
	let files = commit.get("files").and_then(Value::as_array).cloned().unwrap_or_default();
	let commit_payload = object_field(commit, "commit", "commit.commit")?;
	let commit_message = required_string(commit_payload, "message", "commit.commit.message")?;
	let all_patch_text = files
		.iter()
		.filter_map(|file| file.get("patch").and_then(Value::as_str))
		.collect::<Vec<_>>()
		.join("\n");
	let mut bundle_notes = vec!["Built from GitHub commit endpoint without PR context.".to_owned()];

	bundle_notes.extend(notes.iter().cloned());

	let bundle = serde_json::json!({
		"schema": BUNDLE_SCHEMA,
		"repo": repo,
		"analysis_mode": "commit_only",
		"default_branch": default_branch,
		"commits": [commit_bundle_item(&Value::Object(commit.clone()))?],
		"files": files
			.iter()
			.map(file_bundle_item)
			.collect::<crate::prelude::Result<Vec<_>>>()?,
		"linked_issues": collect_issue_refs(&[commit_message])?,
		"extracted_flags": collect_flags(&[commit_message, &all_patch_text])?,
		"docs_refs": collect_docs_refs(&files),
		"examples_refs": collect_examples_refs(&files),
		"notes": bundle_notes,
	});

	validate_bundle_value(&bundle)?;

	Ok(bundle)
}

fn commit_bundle_item(commit: &Value) -> crate::prelude::Result<Value> {
	let commit = object_value(commit, "commit")?;
	let payload = object_field(commit, "commit", "commit.commit")?;
	let author = object_field(payload, "author", "commit.commit.author").ok();
	let author_name = commit
		.get("author")
		.and_then(Value::as_object)
		.and_then(|author| author.get("login"))
		.and_then(Value::as_str)
		.or_else(|| author.and_then(|author| author.get("name")).and_then(Value::as_str));
	let committed_at = author.and_then(|author| author.get("date")).cloned().unwrap_or(Value::Null);

	Ok(serde_json::json!({
		"sha": required_string(commit, "sha", "commit.sha")?,
		"message": first_line(required_string(payload, "message", "commit.commit.message")?),
		"url": required_string(commit, "html_url", "commit.html_url")?,
		"author": author_name,
		"committed_at": committed_at,
	}))
}

fn file_bundle_item(file: &Value) -> crate::prelude::Result<Value> {
	let file = object_value(file, "file")?;

	Ok(serde_json::json!({
		"path": required_string(file, "filename", "file.filename")?,
		"status": required_string(file, "status", "file.status")?,
		"additions": required_i64(file, "additions", "file.additions")?,
		"deletions": required_i64(file, "deletions", "file.deletions")?,
		"patch_excerpt": file
			.get("patch")
			.and_then(Value::as_str)
			.and_then(truncate_patch),
	}))
}

fn validate_bundle_value(bundle: &Value) -> crate::prelude::Result<()> {
	let validation = validate_artifact(bundle);

	if validation.errors.is_empty() && validation.schema.as_deref() == Some(BUNDLE_SCHEMA) {
		Ok(())
	} else {
		let mut errors = validation.errors;

		if validation.schema.as_deref() != Some(BUNDLE_SCHEMA) {
			errors.insert(0, format!("schema must be {BUNDLE_SCHEMA}"));
		}

		eyre::bail!("Bundle validation failed:\n- {}", errors.join("\n- "))
	}
}

fn object_field<'a>(
	object: &'a Map<String, Value>,
	field: &str,
	label: &str,
) -> crate::prelude::Result<&'a Map<String, Value>> {
	object
		.get(field)
		.and_then(Value::as_object)
		.ok_or_else(|| eyre::eyre!("{label} must be an object"))
}

fn required_u64(
	object: &Map<String, Value>,
	field: &str,
	label: &str,
) -> crate::prelude::Result<u64> {
	object
		.get(field)
		.and_then(Value::as_u64)
		.ok_or_else(|| eyre::eyre!("{label} must be an unsigned integer"))
}

fn required_i64(
	object: &Map<String, Value>,
	field: &str,
	label: &str,
) -> crate::prelude::Result<i64> {
	object
		.get(field)
		.and_then(Value::as_i64)
		.ok_or_else(|| eyre::eyre!("{label} must be an integer"))
}

fn pr_labels(pr: &Map<String, Value>) -> Vec<String> {
	pr.get("labels")
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
		.filter_map(|label| {
			label
				.as_object()
				.and_then(|label| label.get("name"))
				.and_then(Value::as_str)
				.map(str::to_owned)
		})
		.collect()
}

fn collect_docs_refs(files: &[Value]) -> Vec<String> {
	files
		.iter()
		.filter_map(file_name)
		.filter(|filename| filename.starts_with("docs/") || filename.ends_with("README.md"))
		.map(str::to_owned)
		.collect()
}

fn collect_examples_refs(files: &[Value]) -> Vec<String> {
	files
		.iter()
		.filter_map(file_name)
		.filter(|filename| {
			filename.to_lowercase().contains("example") || filename.contains("examples/")
		})
		.map(str::to_owned)
		.collect()
}

fn file_name(file: &Value) -> Option<&str> {
	file.as_object()?.get("filename")?.as_str()
}

fn collect_issue_refs(texts: &[&str]) -> crate::prelude::Result<Vec<String>> {
	collect_regex_matches(issue_ref_regex()?, texts)
}

fn collect_flags(texts: &[&str]) -> crate::prelude::Result<Vec<String>> {
	collect_regex_matches(flag_regex()?, texts)
}

fn collect_regex_matches(regex: &Regex, texts: &[&str]) -> crate::prelude::Result<Vec<String>> {
	let mut found = Vec::new();

	for text in texts {
		for captures in regex.captures_iter(text) {
			let Some(value) = captures.get(1).map(|matched| matched.as_str()) else {
				continue;
			};

			if !found.iter().any(|found_value| found_value == value) {
				found.push(value.to_owned());
			}
		}
	}

	Ok(found)
}

fn issue_ref_regex() -> crate::prelude::Result<&'static Regex> {
	static ISSUE_REF_RE: OnceLock<std::result::Result<Regex, regex::Error>> = OnceLock::new();

	ISSUE_REF_RE
		.get_or_init(|| Regex::new(r"(?:^|[^\w])((?:[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+)?#\d+)"))
		.as_ref()
		.map_err(|error| eyre::eyre!("Failed to compile issue reference regex: {error}"))
}

fn flag_regex() -> crate::prelude::Result<&'static Regex> {
	static FLAG_RE: OnceLock<std::result::Result<Regex, regex::Error>> = OnceLock::new();

	FLAG_RE
		.get_or_init(|| {
			Regex::new(r"(?:^|[^\w-])(--[a-zA-Z0-9][\w-]*|[A-Z][A-Z0-9_]{2,}(?:=[^\s,`]+)?)")
		})
		.as_ref()
		.map_err(|error| eyre::eyre!("Failed to compile flag regex: {error}"))
}

fn truncate_patch(value: &str) -> Option<String> {
	let compact = value.trim();

	if compact.is_empty() {
		return None;
	}
	if compact.chars().count() > 900 {
		let mut truncated = compact.chars().take(900).collect::<String>();

		truncated.push_str("...");

		Some(truncated)
	} else {
		Some(compact.into())
	}
}

fn first_line(value: &str) -> String {
	value.trim().lines().next().unwrap_or("").into()
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

fn validate_artifact(payload: &Value) -> ArtifactValidation {
	validate_artifact_with_options(payload, ArtifactValidationOptions::default())
}

fn validate_artifact_for_path(path: &Path, payload: &Value) -> ArtifactValidation {
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

fn has_legacy_multi_agent_v2_context(text: &str) -> bool {
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

fn validate_signal_slug_uniqueness(
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

fn validate_terminal_social_post_idempotency_key_uniqueness(
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

fn validate_active_social_publish_reservation_uniqueness(
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

#[cfg(test)]
mod tests {
	use std::{
		env,
		ffi::OsString,
		fs,
		path::{Path, PathBuf},
		process::Command,
	};

	use serde_json::{self, Value};

	use crate::radar::{
		self, RadarBackfillReleaseRangeRequest, RadarBundleValidateRequest,
		RadarLedgerArtifactLinkRequest, RadarLedgerBootstrapRequest,
		RadarLedgerIngestExistingRequest, RadarRenderSignalRequest,
		RadarSocialReservePublishRequest, RadarValidateRequest, RefreshKind,
	};

	struct TestEnvVars {
		_lock: crate::test_support::TestEnvLockGuard,
		previous: Vec<(String, Option<OsString>)>,
	}

	impl TestEnvVars {
		fn set(vars: &[(&str, Option<&str>)]) -> Self {
			let lock = crate::test_support::TestEnvVarGuard::lock();
			let previous = vars
				.iter()
				.map(|(key, _)| ((*key).to_owned(), env::var_os(key)))
				.collect::<Vec<_>>();

			for (key, value) in vars {
				match value {
					Some(value) => unsafe { env::set_var(key, value) },
					None => unsafe { env::remove_var(key) },
				}
			}

			Self { _lock: lock, previous }
		}
	}

	impl Drop for TestEnvVars {
		fn drop(&mut self) {
			for (key, previous) in self.previous.drain(..).rev() {
				match previous {
					Some(previous) => unsafe { env::set_var(key, previous) },
					None => unsafe { env::remove_var(key) },
				}
			}
		}
	}

	#[test]
	fn accepts_valid_bundle_and_rejects_missing_commits() {
		let mut bundle = valid_bundle();

		assert_errors(&bundle, []);

		bundle["commits"] = serde_json::json!([]);

		assert_errors(&bundle, ["commits must be a non-empty list"]);
	}

	#[test]
	fn accepts_valid_signal_and_rejects_missing_try_effect() {
		let mut signal = valid_signal();

		assert_errors(&signal, []);

		signal["kind"] = serde_json::json!("try_now");
		signal["how_to_try"] = serde_json::json!("Run decodex radar validate.");

		assert_errors(&signal, ["expected_effect is required when how_to_try is present"]);
	}

	#[test]
	fn path_validation_accepts_generated_analysis_drafts_without_schema() {
		let mut draft = serde_json::json!({
			"kind": "behavior_change",
			"title": "Remote control avoids duplicate account headers",
			"summary": "Merged PR centralizes remote-control HTTP auth header construction.",
			"why_it_matters": "Remote-control requests avoid duplicate account headers.",
			"confidence": "confirmed",
			"impact": "low",
			"proof_points": ["The source helper inserts the account header once."],
			"slug": "remote-control-account-header-deduped",
			"config_flags": [],
			"how_to_try": null,
			"expected_effect": null,
			"caveats": null,
			"watch_state": null
		});

		assert_errors(&draft, ["schema must be one of"]);
		assert_path_errors(
			".agent/automations/decodex/cache/generated/analysis/openai-codex-pr-29893.analysis.json",
			&draft,
			[],
		);

		draft["proof_points"] = serde_json::json!([]);

		assert_path_errors(
			".agent/automations/decodex/cache/generated/analysis/openai-codex-pr-29893.analysis.json",
			&draft,
			["proof_points must be a non-empty list"],
		);
	}

	#[test]
	fn rejects_current_multi_agent_v2_signal_assign_task_without_followup_context() {
		let mut signal = valid_signal();

		signal["title"] = serde_json::json!("MultiAgentV2 assign_task guidance");
		signal["summary"] =
			serde_json::json!("MultiAgentV2 operators should use assign_task for more work.");

		assert_errors(
			&signal,
			[
				"MultiAgentV2 assign_task must also mention current followup_task",
				"must describe assign_task as legacy",
			],
		);

		signal["summary"] = serde_json::json!(
			"MultiAgentV2 renamed the legacy assign_task trigger-turn tool to followup_task."
		);

		assert_errors(&signal, []);
	}

	#[test]
	fn validates_multi_agent_v2_feature_catalog_reference() {
		let mut catalog = valid_config_feature_catalog();

		assert_errors(&catalog, []);

		catalog["features"][0]["reference_description"] =
			serde_json::json!("Enable MultiAgentV2 trigger-turn tool assign_task.");

		assert_errors(
			&catalog,
			[
				"reference_description must mention current followup_task behavior",
				"reference_description must label assign_task as legacy or renamed context",
			],
		);
	}

	#[test]
	fn current_multi_agent_v2_references_do_not_require_assign_task() {
		let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
			.parent()
			.and_then(Path::parent)
			.expect("apps/decodex should live two levels under the repo root");
		let mut offenders = Vec::new();

		for relative_root in [
			"README.md",
			"apps/decodex/src",
			"automations/decodex/skills",
			"docs/reference",
			"docs/spec",
			"docs/runbook",
			"plugins/decodex/skills",
			"scripts",
			".agent/automations/decodex/cache/site-content/signals",
			".agent/automations/decodex/cache/generated",
			"site/src/lib",
		] {
			collect_assign_task_reference_violations(
				&repo_root.join(relative_root),
				repo_root,
				&mut offenders,
			);
		}

		assert!(
			offenders.is_empty(),
			"current-facing MultiAgentV2 references must use followup_task and reserve \
			 assign_task for legacy or renamed context: {}",
			offenders.join(", ")
		);
	}

	#[test]
	fn material_refresh_comparison_ignores_only_generated_at() {
		let mut first = valid_release_delta();
		let mut second = first.clone();

		first["generated_at"] = serde_json::json!("2026-06-01T00:00:00Z");
		second["generated_at"] = serde_json::json!("2026-06-02T00:00:00Z");

		assert_eq!(
			radar::material_json(&first, &RefreshKind::ReleaseDelta),
			radar::material_json(&second, &RefreshKind::ReleaseDelta)
		);

		second["stable_release"]["tag_name"] = serde_json::json!("rust-v0.1.1");

		assert_ne!(
			radar::material_json(&first, &RefreshKind::ReleaseDelta),
			radar::material_json(&second, &RefreshKind::ReleaseDelta)
		);
	}

	#[test]
	fn rejects_duplicate_signal_slugs_across_files() {
		let signal = valid_signal();
		let mut state = crate::radar::ValidationState::new();
		let mut errors = Vec::new();

		radar::validate_signal_slug_uniqueness(
			&PathBuf::from(".agent/automations/decodex/cache/site-content/signals/one.json"),
			&signal,
			&mut state,
			&mut errors,
		);
		radar::validate_signal_slug_uniqueness(
			&PathBuf::from(".agent/automations/decodex/cache/site-content/signals/two.json"),
			&signal,
			&mut state,
			&mut errors,
		);

		assert_eq!(errors.len(), 1);
		assert!(errors[0].contains("duplicate slug"));
	}

	#[test]
	fn rejects_duplicate_terminal_social_post_idempotency_keys_across_files() {
		let social_post = valid_social_post();
		let mut state = crate::radar::ValidationState::new();
		let mut errors = Vec::new();

		radar::validate_terminal_social_post_idempotency_key_uniqueness(
			&PathBuf::from(".agent/automations/decodex/cache/social/x/posts/one.json"),
			&social_post,
			&mut state,
			&mut errors,
		);
		radar::validate_terminal_social_post_idempotency_key_uniqueness(
			&PathBuf::from(".agent/automations/decodex/cache/social/x/posts/two.json"),
			&social_post,
			&mut state,
			&mut errors,
		);

		assert_eq!(errors.len(), 1);
		assert!(errors[0].contains("duplicate terminal social_post idempotency_key"));
	}

	#[test]
	fn permits_failed_social_post_idempotency_key_retry() {
		let mut failed_post = valid_social_post();

		failed_post["status"] = serde_json::json!("failed");

		let published_post = valid_social_post();
		let mut state = crate::radar::ValidationState::new();
		let mut errors = Vec::new();

		radar::validate_terminal_social_post_idempotency_key_uniqueness(
			&PathBuf::from(".agent/automations/decodex/cache/social/x/posts/failed.json"),
			&failed_post,
			&mut state,
			&mut errors,
		);
		radar::validate_terminal_social_post_idempotency_key_uniqueness(
			&PathBuf::from(".agent/automations/decodex/cache/social/x/posts/published.json"),
			&published_post,
			&mut state,
			&mut errors,
		);

		assert!(errors.is_empty());
	}

	#[test]
	fn accepts_valid_social_publish_reservation() {
		let reservation = valid_social_publish_reservation();

		assert_errors(&reservation, []);
	}

	#[test]
	fn accepts_valid_radar_archive_manifest() {
		let manifest = valid_radar_archive_manifest();

		assert_errors(&manifest, []);
	}

	#[test]
	fn rejects_radar_archive_manifest_without_external_assets() {
		let mut manifest = valid_radar_archive_manifest();

		manifest["retention_days"] = serde_json::json!(30);

		manifest.as_object_mut().expect("manifest should be object").remove("archive_asset");

		assert_errors(&manifest, ["retention_days must be 21", "archive_asset must be an object"]);
	}

	#[test]
	fn path_validation_accepts_historical_archive_retention_policy() {
		let mut manifest = valid_radar_archive_manifest();

		manifest["created_at"] = serde_json::json!("2026-05-13T07:52:56Z");
		manifest["retention_days"] = serde_json::json!(28);

		assert_errors(&manifest, ["retention_days must be 21"]);
		assert_path_errors(
			".agent/automations/decodex/cache/archive/index/2026-05-13-pre-2026-04-13.json",
			&manifest,
			[],
		);
	}

	#[test]
	fn social_reserve_publish_dry_run_does_not_write() {
		let temp_dir = tempfile::tempdir().expect("temp dir should create");
		let request = social_reserve_request(temp_dir.path(), true);
		let report =
			radar::reserve_social_publish(&request).expect("dry-run reservation should pass");

		assert_eq!(report.status, "dry_run");
		assert!(
			!temp_dir.path().join("reservations/2026-06-02/openai-codex-pr-22414.json").exists(),
			"dry-run should not write reservation"
		);
	}

	#[test]
	fn social_reserve_publish_writes_active_reservation_once() {
		let temp_dir = tempfile::tempdir().expect("temp dir should create");
		let request = social_reserve_request(temp_dir.path(), false);
		let report = radar::reserve_social_publish(&request).expect("reservation should pass");

		assert_eq!(report.status, "reserved");
		assert!(
			temp_dir.path().join("reservations/2026-06-02/openai-codex-pr-22414.json").exists(),
			"reservation should be written"
		);

		let duplicate = radar::reserve_social_publish(&request)
			.expect_err("duplicate reservation should fail closed")
			.to_string();

		assert!(duplicate.contains("idempotency_key already has an active reservation"));
	}

	#[test]
	fn rejects_duplicate_active_social_publish_reservation_idempotency_keys() {
		let reservation = valid_social_publish_reservation();
		let mut state = crate::radar::ValidationState::new();
		let mut errors = Vec::new();

		radar::validate_active_social_publish_reservation_uniqueness(
			&PathBuf::from(".agent/automations/decodex/cache/social/x/reservations/one.json"),
			&reservation,
			&mut state,
			&mut errors,
		);
		radar::validate_active_social_publish_reservation_uniqueness(
			&PathBuf::from(".agent/automations/decodex/cache/social/x/reservations/two.json"),
			&reservation,
			&mut state,
			&mut errors,
		);

		assert_eq!(errors.len(), 1);
		assert!(errors[0].contains("duplicate active social_publish_reservation"));
	}

	#[test]
	fn rejects_active_reservation_for_terminal_social_post_idempotency_key() {
		let social_post = valid_social_post();
		let reservation = valid_social_publish_reservation();
		let mut state = crate::radar::ValidationState::new();
		let mut errors = Vec::new();

		radar::validate_terminal_social_post_idempotency_key_uniqueness(
			&PathBuf::from(".agent/automations/decodex/cache/social/x/posts/published.json"),
			&social_post,
			&mut state,
			&mut errors,
		);
		radar::validate_active_social_publish_reservation_uniqueness(
			&PathBuf::from(".agent/automations/decodex/cache/social/x/reservations/active.json"),
			&reservation,
			&mut state,
			&mut errors,
		);

		assert_eq!(errors.len(), 1);
		assert!(errors[0].contains("conflicts with terminal social_post"));
	}

	#[test]
	fn rejects_terminal_social_post_for_active_reservation_idempotency_key() {
		let social_post = valid_social_post();
		let reservation = valid_social_publish_reservation();
		let mut state = crate::radar::ValidationState::new();
		let mut errors = Vec::new();

		radar::validate_active_social_publish_reservation_uniqueness(
			&PathBuf::from(".agent/automations/decodex/cache/social/x/reservations/active.json"),
			&reservation,
			&mut state,
			&mut errors,
		);
		radar::validate_terminal_social_post_idempotency_key_uniqueness(
			&PathBuf::from(".agent/automations/decodex/cache/social/x/posts/published.json"),
			&social_post,
			&mut state,
			&mut errors,
		);

		assert_eq!(errors.len(), 1);
		assert!(errors[0].contains("conflicts with active reservation"));
	}

	#[test]
	fn accepts_valid_release_delta_and_rejects_missing_default_pair() {
		let mut release_delta = valid_release_delta();

		assert_errors(&release_delta, []);

		release_delta["comparisons"][0]["prerelease_tag_name"] =
			serde_json::json!("rust-v0.2.0-alpha.2");

		assert_errors(
			&release_delta,
			["comparisons must include the default stable/prerelease pair"],
		);
	}

	#[test]
	fn accepts_valid_review_queue_and_rejects_duplicate_subject() {
		let mut queue = valid_review_queue();

		assert_errors(&queue, []);

		queue["subjects"] = serde_json::json!([valid_queue_subject(), valid_queue_subject()]);
		queue["counts"]["subjects_queued"] = serde_json::json!(2);

		assert_errors(&queue, ["duplicates pr:22414"]);
	}

	#[test]
	fn accepts_valid_upstream_review_upgrade_action_and_rejects_stale_action() {
		let mut review = valid_upstream_review();

		assert_errors(&review, []);

		review["next_actions"][0]["type"] = serde_json::json!("control_plane_upgrade_candidate");

		assert_errors(&review, []);

		review["next_actions"][0]["type"] = serde_json::json!("linear_followup");

		assert_errors(&review, ["next_actions[0].type must be one of"]);

		review["next_actions"][0]["type"] = serde_json::json!("publish_now");

		assert_errors(&review, ["next_actions[0].type must be one of"]);
	}

	#[test]
	fn path_validation_accepts_historical_upstream_review_linear_followup_only_before_cutoff() {
		let mut review = valid_upstream_review();

		review["reviewed_at"] = serde_json::json!("2026-06-11T20:07:07Z");
		review["next_actions"][0]["type"] = serde_json::json!("linear_followup");

		assert_errors(&review, ["next_actions[0].type must be one of"]);
		assert_path_errors(
			".agent/automations/decodex/cache/github/reviews/openai-codex-pr-25018.review.json",
			&review,
			[],
		);

		review["reviewed_at"] = serde_json::json!("2026-06-12T00:00:00Z");

		assert_path_errors(
			".agent/automations/decodex/cache/github/reviews/openai-codex-pr-25018.review.json",
			&review,
			["next_actions[0].type must be one of"],
		);
	}

	#[test]
	fn accepts_valid_social_candidate_and_rejects_missing_refs() {
		let mut candidate = valid_social_candidate();

		assert_errors(&candidate, []);

		candidate["source_refs"] = serde_json::json!({});

		assert_errors(&candidate, ["source_refs must include upstream_reviews"]);

		let mut missing_shared_handoff = valid_social_candidate();

		missing_shared_handoff["source_refs"]
			.as_object_mut()
			.expect("source refs should be an object")
			.remove("upstream_impacts");

		assert_errors(
			&missing_shared_handoff,
			["source_refs.upstream_impacts must include the shared upstream_impact/v1 handoff"],
		);
	}

	#[test]
	fn social_candidate_rejects_non_https_source_urls() {
		let mut candidate = valid_social_candidate();

		candidate["source_refs"]["urls"] = serde_json::json!(["http://example.test"]);

		assert_errors(&candidate, ["source_refs.urls must be a list of https URLs"]);
	}

	#[test]
	fn social_candidate_rejects_low_quality_public_text() {
		let mut attribution = valid_social_candidate();

		attribution["candidate_text"] =
			serde_json::json!(["Automated by @hackink: new release available"]);

		assert_errors(&attribution, ["text[0] must not include automation attribution"]);

		let mut overpacked = valid_social_candidate();

		overpacked["candidate_text"] =
			serde_json::json!([format!("{}", "Codex checkpoint ".repeat(18))]);

		assert_errors(&overpacked, ["longer than 260 characters"]);

		let mut generic = valid_social_candidate();

		generic["candidate_text"] = serde_json::json!(["Watching this."]);

		assert_errors(&generic, ["must name a concrete source-backed"]);
	}

	#[test]
	fn accepts_valid_upstream_impact_and_rejects_bad_angle() {
		let mut impact = valid_upstream_impact();

		assert_errors(&impact, []);

		impact["publisher_angle"] = serde_json::json!("viral_thread");

		assert_errors(&impact, ["publisher_angle must be one of"]);
	}

	#[test]
	fn accepts_valid_control_plane_upgrade_candidate_and_rejects_direct_mutation() {
		let mut candidate = valid_control_plane_upgrade_candidate();

		assert_errors(&candidate, []);

		candidate["authority"]["mutation_allowed"] = serde_json::json!(true);

		assert_errors(&candidate, ["authority.mutation_allowed must be false"]);

		let mut missing_shared_handoff = valid_control_plane_upgrade_candidate();

		missing_shared_handoff["source_refs"]
			.as_object_mut()
			.expect("source refs should be an object")
			.remove("upstream_impacts");

		assert_errors(
			&missing_shared_handoff,
			["source_refs.upstream_impacts must include the shared upstream_impact/v1 handoff"],
		);

		let mut missing_contract = valid_control_plane_upgrade_candidate();

		missing_contract["authority"]["decision_contract_required"] = serde_json::json!(false);

		assert_errors(&missing_contract, ["authority.decision_contract_required must be true"]);

		let mut missing_program = valid_control_plane_upgrade_candidate();

		missing_program["authority"]
			.as_object_mut()
			.expect("authority should be an object")
			.remove("program_intake_required");

		assert_errors(&missing_program, ["authority.program_intake_required must be true"]);
	}

	#[test]
	fn accepts_valid_social_post_and_rejects_bad_daily_limit() {
		let mut social_post = valid_social_post();

		assert_errors(&social_post, []);

		social_post["decision"]["daily_limit"] = serde_json::json!(9);

		assert_errors(&social_post, ["decision.daily_limit must be 8"]);
	}

	#[test]
	fn social_post_rejects_low_quality_public_text() {
		let mut attribution = valid_social_post();

		attribution["text"] = serde_json::json!(["Automated by @hackink: tracking this."]);

		assert_errors(&attribution, ["text[0] must not include automation attribution"]);

		let mut overpacked = valid_social_post();

		overpacked["text"] = serde_json::json!([format!("{}", "Codex checkpoint ".repeat(18))]);

		assert_errors(&overpacked, ["longer than 260 characters"]);

		let mut with_source_url = valid_social_post();

		with_source_url["text"] = serde_json::json!([format!(
			"{} https://github.com/openai/codex/pull/22414",
			"Codex checkpoint ".repeat(13)
		)]);

		assert_errors(&with_source_url, []);
	}

	#[test]
	fn accepts_deleted_social_post_lifecycle_and_rejects_quote_eligible_deleted_post() {
		let mut social_post = valid_social_post();

		social_post["post_lifecycle"] = serde_json::json!({
			"current_state": "deleted_by_operator",
			"quote_eligible": false,
			"superseded_by_candidate": ".agent/automations/decodex/cache/github/social-candidates/openai-codex-alpha4.json",
			"reason": "The operator deleted this post and superseded it with a corrected candidate."
		});

		assert_errors(&social_post, []);

		social_post["post_lifecycle"]["quote_eligible"] = serde_json::json!(true);

		assert_errors(
			&social_post,
			["post_lifecycle.quote_eligible can be true only for live published posts"],
		);
	}

	#[test]
	fn default_github_token_falls_back_to_workflow_token() {
		let _env = TestEnvVars::set(&[
			("GITHUB_PAT_X", Some("")),
			("GITHUB_PAT_Y", Some("")),
			("GITHUB_TOKEN", Some("workflow-token")),
		]);

		assert_eq!(super::github_token(None).as_deref(), Some("workflow-token"));
	}

	#[test]
	fn explicit_github_token_env_does_not_fall_back_to_workflow_token() {
		let _env = TestEnvVars::set(&[
			("DECODEX_TEST_MISSING_RADAR_TOKEN", None),
			("GITHUB_TOKEN", Some("workflow-token")),
		]);

		assert_eq!(super::github_token(Some("DECODEX_TEST_MISSING_RADAR_TOKEN")), None);
	}

	#[test]
	fn validates_json_files_from_directory() {
		let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
		let path = temp_dir.path().join("bundle.json");

		fs::write(&path, valid_bundle().to_string()).expect("fixture should be written");

		let report =
			radar::validate(&RadarValidateRequest { paths: vec![temp_dir.path().to_path_buf()] })
				.expect("valid temporary bundle should pass");

		assert_eq!(report.checked_files, 1);
	}

	#[test]
	fn renders_signal_from_bundle_and_analysis_fixture() {
		let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
		let bundle_path = temp_dir.path().join("bundle.json");
		let analysis_path = temp_dir.path().join("analysis.json");
		let signal_path = temp_dir.path().join("signal.json");
		let analysis = serde_json::json!({
			"kind": "capability",
			"title": "Unix sockets for remote Codex",
			"summary": "Remote Codex can use Unix socket endpoints.",
			"why_it_matters": "Operators can use local socket transports.",
			"confidence": "confirmed",
			"impact": "medium",
			"proof_points": ["PR #22414 adds endpoint handling."],
			"slug": null,
			"config_flags": [],
			"how_to_try": null,
			"expected_effect": null,
			"caveats": null,
			"watch_state": null
		});

		fs::write(&bundle_path, valid_bundle().to_string()).expect("bundle should be written");
		fs::write(&analysis_path, analysis.to_string()).expect("analysis should be written");

		let report = radar::render_signal(&RadarRenderSignalRequest {
			bundle: bundle_path,
			analysis: analysis_path,
			out: signal_path.clone(),
			published_at: None,
		})
		.expect("rendered signal should pass validation");
		let rendered: Value = serde_json::from_str(
			&fs::read_to_string(&signal_path).expect("rendered signal should be readable"),
		)
		.expect("rendered signal should parse");

		assert_eq!(report.out, signal_path);
		assert_eq!(rendered["schema"], "signal_entry/v1");
		assert_eq!(rendered["slug"], "unix-sockets-for-remote-codex");
		assert_eq!(rendered["published_at"], "2026-06-01T00:00:00Z");
		assert_eq!(rendered["source_refs"]["items"][0]["meta"], serde_json::json!("#22414"));
		assert_eq!(rendered["source_refs"]["items"][1]["meta"], "abc123");
		assert!(rendered.get("how_to_try").is_none());
	}

	#[test]
	fn analysis_helper_fails_closed_without_explicit_boundary_opt_in() {
		let _env = TestEnvVars::set(&[("DECODEX_ALLOW_CODEX_ANALYSIS", None)]);
		let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
			.parent()
			.and_then(Path::parent)
			.expect("apps/decodex should live two levels under the repo root");
		let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
		let bundle_path = temp_dir.path().join("missing-bundle.json");
		let output_path = temp_dir.path().join("analysis.json");
		let output = Command::new("python3")
			.current_dir(repo_root)
			.arg(repo_root.join(super::RUN_CODEX_ANALYSIS_SCRIPT))
			.arg("--bundle")
			.arg(&bundle_path)
			.arg("--out")
			.arg(&output_path)
			.arg("--repo-root")
			.arg(repo_root)
			.output()
			.expect("Python analysis helper smoke command should execute");
		let stderr = String::from_utf8_lossy(&output.stderr);

		assert!(!output.status.success());
		assert!(
			stderr.contains("requires --allow-ai-analysis-boundary"),
			"unexpected stderr: {stderr}"
		);
		assert!(!output_path.exists());
	}

	#[test]
	fn dry_run_backfill_selects_unpublished_release_window_prs() {
		let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
		let release_delta_path = temp_dir.path().join("release-delta.json");
		let signals_dir = temp_dir.path().join("signals");
		let mut release_delta = valid_release_delta();

		release_delta["compare"]["pr_numbers"] = serde_json::json!([22_414, 22_415, 22_416]);
		release_delta["comparisons"][0]["compare"]["pr_numbers"] =
			serde_json::json!([22_414, 22_415, 22_416]);

		fs::create_dir_all(&signals_dir).expect("signals directory should be created");
		fs::write(release_delta_path.as_path(), release_delta.to_string())
			.expect("release delta should be written");
		fs::write(signals_dir.join("published.json"), valid_signal().to_string())
			.expect("signal should be written");

		let report = radar::backfill_release_range(&RadarBackfillReleaseRangeRequest {
			repo: "openai/codex".into(),
			release_delta: release_delta_path,
			stable_tag: None,
			preview_tag: None,
			signals_dir,
			bundles_dir: temp_dir.path().join("bundles"),
			analysis_dir: temp_dir.path().join("analysis"),
			token_env: None,
			codex_bin: "codex".into(),
			model: None,
			max_prs: Some(1),
			dry_run: true,
			refresh_release_delta_first: false,
			refresh_stable_limit: None,
			refresh_preview_limit: None,
			refresh_pair_limit: None,
			python_bin: "python3".into(),
		})
		.expect("dry-run backfill should select unpublished PRs");

		assert_eq!(report.stable_tag, "rust-v0.1.0");
		assert_eq!(report.preview_tag, "rust-v0.2.0-alpha.1");
		assert_eq!(report.target_prs, vec![22_415]);
		assert_eq!(report.created, 0);
		assert!(report.dry_run);
	}

	#[test]
	fn ledger_bootstrap_migrates_social_draft_artifact_kind() {
		let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
		let db_path = temp_dir.path().join("radar.sqlite3");
		let connection =
			rusqlite::Connection::open(&db_path).expect("temporary ledger should open");

		connection
			.execute_batch(
				"
				CREATE TABLE artifact_link (
				  repo TEXT NOT NULL,
				  subject_kind TEXT NOT NULL CHECK (subject_kind IN ('commit', 'pr')),
				  subject_id TEXT NOT NULL,
				  artifact_kind TEXT NOT NULL CHECK (
				    artifact_kind IN (
				      'bundle',
				      'analysis',
				      'signal',
				      'upstream_impact',
				      'social_draft',
				      'release_delta',
				      'archive_manifest',
				      'ledger_export'
				    )
				  ),
				  path TEXT NOT NULL,
				  sha256 TEXT NOT NULL,
				  size_bytes INTEGER NOT NULL,
				  created_at TEXT NOT NULL,
				  PRIMARY KEY (repo, subject_kind, subject_id, artifact_kind, path)
				);
				INSERT INTO artifact_link (
				  repo,
				  subject_kind,
				  subject_id,
				  artifact_kind,
				  path,
				  sha256,
				  size_bytes,
				  created_at
				)
				VALUES (
				  'openai/codex',
				  'pr',
				  '22414',
				  'social_draft',
				  '.agent/automations/decodex/cache/social/x/posts/2026-06-01/example.json',
				  'abc123',
				  10,
				  '2026-06-01T00:00:00Z'
				);
				",
			)
			.expect("legacy artifact link schema should be created");

		drop(connection);

		radar::ledger_bootstrap(&RadarLedgerBootstrapRequest { db_path: db_path.clone() })
			.expect("ledger bootstrap should migrate social_draft rows");

		let connection = rusqlite::Connection::open(&db_path).expect("migrated ledger should open");
		let artifact_kind: String = connection
			.query_row("SELECT artifact_kind FROM artifact_link", [], |row| row.get(0))
			.expect("artifact kind should be readable");
		let schema_version: String = connection
			.query_row("SELECT value FROM metadata WHERE key = 'schema_version'", [], |row| {
				row.get(0)
			})
			.expect("schema version should be readable");

		assert_eq!(artifact_kind, "social_post");
		assert_eq!(schema_version, "4");
	}

	#[test]
	fn ledger_ingests_existing_bundle_analysis_and_signal_artifacts() {
		let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
		let bundles_dir = temp_dir.path().join("bundles");
		let analysis_dir = temp_dir.path().join("analysis");
		let signals_dir = temp_dir.path().join("signals");
		let db_path = temp_dir.path().join("radar.sqlite3");

		fs::create_dir_all(&bundles_dir).expect("bundles directory should be created");
		fs::create_dir_all(&analysis_dir).expect("analysis directory should be created");
		fs::create_dir_all(&signals_dir).expect("signals directory should be created");
		fs::write(bundles_dir.join("openai-codex-pr-22414.json"), valid_bundle().to_string())
			.expect("bundle fixture should be written");
		fs::write(
			analysis_dir.join("openai-codex-pr-22414.analysis.json"),
			r#"{"kind":"capability"}"#,
		)
		.expect("analysis fixture should be written");
		fs::write(signals_dir.join("openai-codex-pr-22414.json"), valid_signal().to_string())
			.expect("signal fixture should be written");

		let summary = radar::ledger_ingest_existing(&RadarLedgerIngestExistingRequest {
			db_path: db_path.clone(),
			bundles_dir,
			analysis_dir,
			signals_dir,
		})
		.expect("existing artifacts should ingest");

		assert_eq!(summary.get("bundles_ingested"), Some(&1));
		assert_eq!(summary.get("upstream_commits"), Some(&1));
		assert_eq!(summary.get("radar_reviews"), Some(&1));
		assert_eq!(summary.get("artifact_links"), Some(&3));

		let connection = rusqlite::Connection::open(&db_path).expect("ingested ledger should open");
		let review: (String, String) = connection
			.query_row(
				"SELECT status, confidence FROM radar_review WHERE subject_kind = 'pr'",
				[],
				|row| Ok((row.get(0)?, row.get(1)?)),
			)
			.expect("review row should be readable");

		assert_eq!(review, ("signal".into(), "confirmed".into()));
	}

	#[test]
	fn ledger_artifact_link_records_social_post_artifacts() {
		let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
		let db_path = temp_dir.path().join("radar.sqlite3");
		let social_post_path = temp_dir.path().join("post.json");

		fs::write(&social_post_path, r#"{"schema":"social_post/v1"}"#)
			.expect("social post fixture should be written");

		let summary = radar::ledger_artifact_link(&RadarLedgerArtifactLinkRequest {
			db_path: db_path.clone(),
			repo: "openai/codex".into(),
			subject_kind: "pr".into(),
			subject_id: "22414".into(),
			artifact_kind: "social_post".into(),
			path: social_post_path,
		})
		.expect("artifact link should be recorded");

		assert_eq!(summary.get("artifact_links"), Some(&1));

		let connection =
			rusqlite::Connection::open(&db_path).expect("ledger should open after artifact link");
		let artifact_kind: String = connection
			.query_row("SELECT artifact_kind FROM artifact_link", [], |row| row.get(0))
			.expect("artifact link row should be readable");

		assert_eq!(artifact_kind, "social_post");
	}

	#[test]
	fn ledger_artifact_link_records_social_candidate_after_schema_migration() {
		let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
		let db_path = temp_dir.path().join("radar.sqlite3");
		let social_candidate_path = temp_dir.path().join("candidate.json");
		let connection =
			rusqlite::Connection::open(&db_path).expect("temporary ledger should open");

		connection
			.execute_batch(
				"
				CREATE TABLE artifact_link (
				  repo TEXT NOT NULL,
				  subject_kind TEXT NOT NULL CHECK (subject_kind IN ('commit', 'pr')),
				  subject_id TEXT NOT NULL,
				  artifact_kind TEXT NOT NULL CHECK (
				    artifact_kind IN (
				      'bundle',
				      'analysis',
				      'signal',
				      'upstream_impact',
				      'social_post',
				      'release_delta',
				      'archive_manifest',
				      'ledger_export'
				    )
				  ),
				  path TEXT NOT NULL,
				  sha256 TEXT NOT NULL,
				  size_bytes INTEGER NOT NULL,
				  created_at TEXT NOT NULL,
				  PRIMARY KEY (repo, subject_kind, subject_id, artifact_kind, path)
				);
				",
			)
			.expect("legacy artifact link schema should be created");

		drop(connection);

		fs::write(&social_candidate_path, r#"{"schema":"social_candidate/v1"}"#)
			.expect("social candidate fixture should be written");
		radar::ledger_bootstrap(&RadarLedgerBootstrapRequest { db_path: db_path.clone() })
			.expect("ledger bootstrap should add social_candidate artifact kind");

		let summary = radar::ledger_artifact_link(&RadarLedgerArtifactLinkRequest {
			db_path: db_path.clone(),
			repo: "openai/codex".into(),
			subject_kind: "pr".into(),
			subject_id: "22414".into(),
			artifact_kind: "social_candidate".into(),
			path: social_candidate_path,
		})
		.expect("social candidate artifact link should be recorded");

		assert_eq!(summary.get("artifact_links"), Some(&1));
	}

	#[test]
	fn ledger_artifact_link_records_control_plane_upgrade_candidate_after_schema_migration() {
		let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
		let db_path = temp_dir.path().join("radar.sqlite3");
		let candidate_path = temp_dir.path().join("upgrade.json");
		let connection =
			rusqlite::Connection::open(&db_path).expect("temporary ledger should open");

		connection
			.execute_batch(
				"
				CREATE TABLE artifact_link (
				  repo TEXT NOT NULL,
				  subject_kind TEXT NOT NULL CHECK (subject_kind IN ('commit', 'pr')),
				  subject_id TEXT NOT NULL,
				  artifact_kind TEXT NOT NULL CHECK (
				    artifact_kind IN (
				      'bundle',
				      'analysis',
				      'signal',
				      'upstream_impact',
				      'social_candidate',
				      'social_post',
				      'release_delta',
				      'archive_manifest',
				      'ledger_export'
				    )
				  ),
				  path TEXT NOT NULL,
				  sha256 TEXT NOT NULL,
				  size_bytes INTEGER NOT NULL,
				  created_at TEXT NOT NULL,
				  PRIMARY KEY (repo, subject_kind, subject_id, artifact_kind, path)
				);
				",
			)
			.expect("legacy artifact link schema should be created");

		drop(connection);

		fs::write(&candidate_path, r#"{"schema":"control_plane_upgrade_candidate/v1"}"#)
			.expect("upgrade candidate fixture should be written");
		radar::ledger_bootstrap(&RadarLedgerBootstrapRequest { db_path: db_path.clone() })
			.expect("ledger bootstrap should add control-plane upgrade artifact kind");

		let summary = radar::ledger_artifact_link(&RadarLedgerArtifactLinkRequest {
			db_path: db_path.clone(),
			repo: "openai/codex".into(),
			subject_kind: "pr".into(),
			subject_id: "22414".into(),
			artifact_kind: "control_plane_upgrade_candidate".into(),
			path: candidate_path,
		})
		.expect("control-plane upgrade artifact link should be recorded");

		assert_eq!(summary.get("artifact_links"), Some(&1));
	}

	#[test]
	fn builds_pr_bundle_from_fixture_payloads() {
		let patch = format!("{} --config FEATURE_FLAG=1", "a".repeat(910));
		let pr = serde_json::json!({
			"number": 22_414,
			"title": "Add Unix socket endpoint support",
			"body": "Fixes #123 and enables --sandbox.",
			"state": "closed",
			"merged_at": "2026-06-01T00:00:00Z",
			"labels": [{"name": "enhancement"}],
			"html_url": "https://github.com/openai/codex/pull/22414"
		});
		let commits = vec![serde_json::json!({
			"sha": "abc123",
			"html_url": "https://github.com/openai/codex/commit/abc123",
			"author": {"login": "alice"},
			"commit": {
				"message": "Add Unix socket endpoint support\n\nRefs openai/codex#456",
				"author": {
					"name": "Alice",
					"date": "2026-06-01T00:00:00Z"
				}
			}
		})];
		let files = vec![serde_json::json!({
			"filename": "docs/examples/socket.md",
			"status": "modified",
			"additions": 12,
			"deletions": 1,
			"patch": patch
		})];
		let bundle = super::build_pr_bundle_from_sources(
			"openai/codex",
			&pr,
			&commits,
			&files,
			"main",
			&["fixture note".into()],
		)
		.expect("PR bundle should build from fixture payloads");

		assert_errors(&bundle, []);

		assert_eq!(bundle["analysis_mode"], "pr_first");
		assert_eq!(bundle["primary_pr"]["state"], "merged");
		assert_eq!(bundle["primary_pr"]["labels"], serde_json::json!(["enhancement"]));
		assert_eq!(bundle["linked_issues"], serde_json::json!(["#123", "openai/codex#456"]));
		assert_eq!(
			bundle["extracted_flags"],
			serde_json::json!(["--sandbox", "--config", "FEATURE_FLAG=1"])
		);
		assert_eq!(bundle["docs_refs"], serde_json::json!(["docs/examples/socket.md"]));
		assert_eq!(bundle["examples_refs"], serde_json::json!(["docs/examples/socket.md"]));
		assert_eq!(bundle["notes"][1], "fixture note");

		let patch_excerpt =
			bundle["files"][0]["patch_excerpt"].as_str().expect("patch excerpt should be present");

		assert!(patch_excerpt.ends_with("..."));
		assert_eq!(patch_excerpt.chars().count(), 903);
	}

	#[test]
	fn validates_bundle_directories_and_rejects_other_schemas() {
		let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
		let bundle_path = temp_dir.path().join("bundle.json");
		let signal_path = temp_dir.path().join("signal.json");

		fs::write(&bundle_path, valid_bundle().to_string()).expect("bundle should be written");

		let report = radar::validate_bundles(&RadarBundleValidateRequest {
			paths: vec![temp_dir.path().to_path_buf()],
		})
		.expect("bundle directory should validate");

		assert_eq!(report.checked_files, 1);

		fs::write(&signal_path, valid_signal().to_string()).expect("signal should be written");

		let error = radar::validate_bundles(&RadarBundleValidateRequest {
			paths: vec![temp_dir.path().to_path_buf()],
		})
		.expect_err("non-bundle schema should be rejected by bundle validation");
		let message = error.to_string();

		assert!(message.contains("schema must be github_change_bundle/v1"));
	}

	fn assert_errors<const N: usize>(payload: &Value, expected: [&str; N]) {
		let validation = radar::validate_artifact(payload);

		for expected_error in expected {
			assert!(
				validation.errors.iter().any(|error| error.contains(expected_error)),
				"expected error containing {expected_error:?}, got {:?}",
				validation.errors
			);
		}

		if expected.is_empty() {
			assert_eq!(validation.errors, Vec::<String>::new());
		}
	}

	fn assert_path_errors<const N: usize>(path: &str, payload: &Value, expected: [&str; N]) {
		let validation = radar::validate_artifact_for_path(Path::new(path), payload);

		for expected_error in expected {
			assert!(
				validation.errors.iter().any(|error| error.contains(expected_error)),
				"expected error containing {expected_error:?}, got {:?}",
				validation.errors
			);
		}

		if expected.is_empty() {
			assert_eq!(validation.errors, Vec::<String>::new());
		}
	}

	fn valid_bundle() -> Value {
		serde_json::json!({
			"schema": "github_change_bundle/v1",
			"repo": "openai/codex",
			"analysis_mode": "pr_first",
			"default_branch": "main",
			"primary_pr": {
				"number": 22_414,
				"title": "Add Unix socket endpoint support",
				"body": "",
				"state": "merged",
				"merged_at": "2026-06-01T00:00:00Z",
				"labels": [],
				"url": "https://github.com/openai/codex/pull/22414"
			},
			"commits": [
				{
					"sha": "abc123",
					"message": "Add Unix socket endpoint support",
					"url": "https://github.com/openai/codex/commit/abc123"
				}
			],
			"files": [
				{
					"path": "codex-rs/app-server/src/lib.rs",
					"status": "modified",
					"additions": 12,
					"deletions": 1
				}
			]
		})
	}

	fn valid_signal() -> Value {
		serde_json::json!({
			"schema": "signal_entry/v1",
			"slug": "openai-codex-pr-22414",
			"lane": "github",
			"kind": "capability",
			"title": "Unix sockets for remote Codex",
			"published_at": "2026-06-01T00:00:00Z",
			"summary": "Remote Codex can use Unix socket endpoints.",
			"why_it_matters": "Operators can use local socket transports.",
			"confidence": "confirmed",
			"impact": "medium",
			"proof_points": ["PR #22414 adds endpoint handling."],
			"source_refs": {
				"repo": "openai/codex",
				"pr_url": "https://github.com/openai/codex/pull/22414",
				"items": [
					{
						"kind": "pull_request",
						"title": "Add Unix socket endpoint support",
						"url": "https://github.com/openai/codex/pull/22414"
					}
				]
			}
		})
	}

	fn valid_config_feature_catalog() -> Value {
		serde_json::json!({
			"schema": "codex_config_feature_catalog/v1",
			"source_url": "https://raw.githubusercontent.com/openai/codex/main/codex-rs/core/config.schema.json",
			"generated_at": "2026-06-02T00:00:00Z",
			"feature_count": 1,
			"features": [
				{
					"name": "multi_agent_v2",
					"config_path": "features.multi_agent_v2",
					"toml_assignment": "multi_agent_v2 = true",
					"toml_snippet": "[features]\nmulti_agent_v2 = true",
					"cli_enable_flag": "--enable multi_agent_v2",
					"schema_url": "https://raw.githubusercontent.com/openai/codex/main/codex-rs/core/config.schema.json",
					"reference_url": "https://developers.openai.com/codex/config-reference",
					"reference_description": "Enable MultiAgentV2 tools including followup_task; legacy assign_task appears only in older rollout traces.",
					"github_search_url": "https://github.com/openai/codex/search?q=%22multi_agent_v2%22&type=code"
				}
			]
		})
	}

	fn collect_assign_task_reference_violations(
		path: &Path,
		repo_root: &Path,
		offenders: &mut Vec<String>,
	) {
		let Ok(metadata) = fs::metadata(path) else {
			return;
		};

		if metadata.is_dir() {
			let entries = fs::read_dir(path).expect("reference audit directory should be readable");

			for entry in entries {
				let entry = entry.expect("reference audit directory entry should be readable");

				collect_assign_task_reference_violations(&entry.path(), repo_root, offenders);
			}

			return;
		}
		if !metadata.is_file() || !should_audit_multi_agent_v2_reference_file(path) {
			return;
		}

		let text = fs::read_to_string(path).expect("reference audit file should be utf-8 text");
		let lower = text.to_ascii_lowercase();

		if !lower.contains("assign_task") {
			return;
		}
		if lower.contains("followup_task") && radar::has_legacy_multi_agent_v2_context(&lower) {
			return;
		}

		let relative = path.strip_prefix(repo_root).unwrap_or(path);

		offenders.push(relative.display().to_string());
	}

	fn should_audit_multi_agent_v2_reference_file(path: &Path) -> bool {
		let extension = path.extension().and_then(|value| value.to_str());

		matches!(extension, Some("json" | "md" | "py" | "rs" | "ts" | "tsx"))
	}

	fn valid_release_delta() -> Value {
		serde_json::json!({
			"schema": "release_delta/v1",
			"repo": "openai/codex",
			"tag_prefix": "rust-v",
			"generated_at": "2026-06-01T00:00:00Z",
			"stable_release": release("rust-v0.1.0", false),
			"prerelease": release("rust-v0.2.0-alpha.1", true),
			"compare": compare(),
			"tracked_signal_slugs": ["openai-codex-pr-22414"],
			"release_options": {
				"stable": [release("rust-v0.1.0", false)],
				"preview": [release("rust-v0.2.0-alpha.1", true)]
			},
			"comparisons": [
				{
					"stable_tag_name": "rust-v0.1.0",
					"prerelease_tag_name": "rust-v0.2.0-alpha.1",
					"compare": compare(),
					"tracked_signal_slugs": ["openai-codex-pr-22414"]
				}
			]
		})
	}

	fn release(tag_name: &str, prerelease: bool) -> Value {
		serde_json::json!({
			"tag_name": tag_name,
			"name": tag_name,
			"published_at": "2026-06-01T00:00:00Z",
			"url": "https://github.com/openai/codex/releases/tag/rust-v0.1.0",
			"prerelease": prerelease
		})
	}

	fn compare() -> Value {
		serde_json::json!({
			"status": "ahead",
			"ahead_by": 1,
			"total_commits": 1,
			"url": "https://github.com/openai/codex/compare/rust-v0.1.0...rust-v0.2.0-alpha.1",
			"commit_shas": ["abc123"],
			"pr_numbers": [22_414]
		})
	}

	fn valid_review_queue() -> Value {
		serde_json::json!({
			"schema": "upstream_review_queue/v1",
			"repo": "openai/codex",
			"generated_at": "2026-06-01T00:00:00Z",
			"source": {
				"default_branch": "main",
				"search_limit": 40
			},
			"subjects": [valid_queue_subject()],
			"counts": {
				"subjects_queued": 1,
				"recent_commits_scanned": 1,
				"published_subjects_seen": 0,
				"critical": 0,
				"high": 1,
				"normal": 0,
				"low": 0
			}
		})
	}

	fn valid_queue_subject() -> Value {
		serde_json::json!({
			"subject_kind": "pr",
			"subject_id": "22414",
			"title": "Add Unix socket endpoint support",
			"url": "https://github.com/openai/codex/pull/22414",
			"source_state": "merged",
			"commit_shas": ["abc123"],
			"changed_file_count": 1,
			"sample_paths": ["codex-rs/app-server/src/lib.rs"],
			"surface_hints": ["app_server_protocol"],
			"attention_flags": [],
			"review_priority": "high",
			"review_reason": "Transport behavior changed.",
			"next_step": "ai_review_required"
		})
	}

	fn valid_upstream_review() -> Value {
		serde_json::json!({
			"schema": "upstream_review/v1",
			"slug": "openai-codex-pr-22414",
			"repo": "openai/codex",
			"subject": {
				"subject_kind": "pr",
				"subject_id": "22414",
				"commit_shas": ["abc123"]
			},
			"source_refs": {
				"items": [
					{
						"kind": "pull_request",
						"title": "Add Unix socket endpoint support",
						"url": "https://github.com/openai/codex/pull/22414"
					}
				]
			},
			"reviewed_at": "2026-06-01T00:00:00Z",
			"observed_change": "Remote Codex can use Unix socket endpoints.",
			"changed_surfaces": ["app server"],
			"confidence": "confirmed",
			"evidence": ["PR #22414 updates app-server endpoint handling."],
			"next_actions": [
				{
					"type": "upstream_impact",
					"reason": "Transport behavior can affect Decodex."
				}
			]
		})
	}

	fn valid_upstream_impact() -> Value {
		serde_json::json!({
			"schema": "upstream_impact/v1",
			"slug": "openai-codex-pr-22414",
			"repo": "openai/codex",
			"source_refs": {
				"items": [
					{
						"kind": "pull_request",
						"title": "Add Unix socket endpoint support",
						"url": "https://github.com/openai/codex/pull/22414"
					}
				]
			},
			"observed_change": "Remote Codex can use Unix socket endpoints.",
			"public_signal_decision": "publish",
			"control_plane_impact": "candidate",
			"publisher_angle": "operator_impact",
			"confidence": "confirmed",
			"evidence": ["PR #22414 updates app-server endpoint handling."]
		})
	}

	fn valid_control_plane_upgrade_candidate() -> Value {
		serde_json::json!({
			"schema": "control_plane_upgrade_candidate/v1",
			"slug": "openai-codex-pr-22414-control-plane",
			"repo": "openai/codex",
			"status": "proposed",
			"source_refs": {
				"upstream_reviews": [
					".agent/automations/decodex/cache/github/reviews/openai-codex-pr-22414.review.json"
				],
				"upstream_impacts": [
					".agent/automations/decodex/cache/github/impact/openai-codex-pr-22414.json"
				],
				"urls": ["https://github.com/openai/codex/pull/22414"]
			},
			"observed_change": "Remote Codex can use Unix socket endpoints.",
			"control_plane_impact": "compat_risk",
			"upgrade_path": "compat_risk_mitigation",
			"affected_surfaces": ["app-server protocol"],
			"target_codex": {
				"channel": "stable",
				"version": "0.142.2",
				"tag": "rust-v0.142.2",
				"release_url": "https://github.com/openai/codex/releases/tag/rust-v0.142.2",
				"compatibility_status": "needs_review",
				"matrix_ref": "docs/reference/codex-compatibility-matrix.md#codex-01422"
			},
			"authority": {
				"decision_contract_required": true,
				"program_intake_required": true,
				"mutation_allowed": false,
				"objective_id": "decodex-self-iteration"
			},
			"reason": "The upstream app-server transport change may affect Decodex Control Plane compatibility.",
			"validation_gates": ["decodex probe stdio://", "cargo test -p decodex app_server --lib"],
			"stop_conditions": ["Missing accepted Decision Contract", "Probe failure against the target Codex build"],
			"acceptance_criteria": [
				"Compatibility impact is proven or dismissed with source-backed evidence."
			]
		})
	}

	fn valid_social_candidate() -> Value {
		serde_json::json!({
			"schema": "social_candidate/v1",
			"slug": "openai-codex-pr-22414",
			"repo": "openai/codex",
			"channel": "x",
			"target_account": "decodexspace",
			"mode": "operator_impact",
			"priority": "normal",
			"audience": "Codex operators",
			"candidate_text": [
				"Remote Codex can now use Unix socket endpoints. Source: https://github.com/openai/codex/pull/22414"
			],
			"source_refs": {
				"upstream_reviews": [".agent/automations/decodex/cache/github/reviews/openai-codex-pr-22414.review.json"],
				"upstream_impacts": [".agent/automations/decodex/cache/github/impact/openai-codex-pr-22414.json"],
				"urls": ["https://github.com/openai/codex/pull/22414"]
			},
			"evidence_notes": ["PR #22414 changes remote endpoint handling."],
			"claims": [
				{
					"text": "Remote Codex can use Unix socket endpoints.",
					"evidence": "https://github.com/openai/codex/pull/22414",
					"confidence": "confirmed"
				}
			],
			"decision": {
				"worthiness": "publish",
				"reason": "The source-backed review has a clear operator impact angle.",
				"idempotency_key": "x:decodexspace:openai-codex-pr-22414:operator_impact"
			}
		})
	}

	fn valid_social_post() -> Value {
		serde_json::json!({
			"schema": "social_post/v1",
			"slug": "openai-codex-pr-22414",
			"channel": "x",
			"target_account": "decodexspace",
			"controller_account": "hackink",
			"mode": "operator_impact",
			"status": "published",
			"audience": "Codex operators",
			"text": ["Remote Codex can now use Unix socket endpoints. Source: https://github.com/openai/codex/pull/22414"],
			"source_refs": {
				"urls": ["https://github.com/openai/codex/pull/22414"]
			},
			"evidence_notes": ["PR #22414 changes remote endpoint handling."],
			"claims": [
				{
					"text": "Remote Codex can use Unix socket endpoints.",
					"evidence": "https://github.com/openai/codex/pull/22414",
					"confidence": "confirmed"
				}
			],
			"decision": {
				"worthiness": "publish",
				"priority": "high",
				"idempotency_key": "x:decodexspace:operator_impact:openai-codex-pr-22414",
				"reason": "High-value Control Plane transport implication.",
				"daily_limit": 8,
				"daily_count_before": 2,
				"daily_count_after": 3,
				"day": "2026-06-02",
				"timezone": "Asia/Shanghai"
			},
			"publication": {
				"posted_at": "2026-06-02T03:00:00Z",
				"published_urls": ["https://x.com/decodexspace/status/1"],
				"publisher": "chrome",
				"account_verified": true,
				"made_with_ai": true,
				"image_template": "decodex_signal_card"
			},
			"media_refs": ["https://x.com/decodexspace/status/1/photo/1"]
		})
	}

	fn valid_social_publish_reservation() -> Value {
		serde_json::json!({
			"schema": "social_publish_reservation/v1",
			"slug": "openai-codex-pr-22414",
			"channel": "x",
			"target_account": "decodexspace",
			"controller_account": "hackink",
			"mode": "operator_impact",
			"status": "active",
			"idempotency_key": "x:decodexspace:operator_impact:openai-codex-pr-22414",
			"reserved_at": "2026-06-02T02:50:00Z",
			"expires_at": "2026-06-02T03:50:00Z",
			"day": "2026-06-02",
			"timezone": "Asia/Shanghai",
			"candidate_refs": {
				"social_candidates": [
					".agent/automations/decodex/cache/github/social-candidates/openai-codex-pr-22414.json"
				],
				"urls": ["https://github.com/openai/codex/pull/22414"]
			},
			"duplicate_keys": [
				"Remote Codex can now use Unix socket endpoints.",
				"https://github.com/openai/codex/pull/22414"
			],
			"owner": {
				"automation_id": "decodex-x-publisher",
				"branch": "automation/decodex-x-publisher-2026-06-02-pr-22414",
				"pr_url": "https://github.com/hack-ink/decodex/pull/1",
				"run_id": "2026-06-02T02:50:00Z"
			},
			"evidence_notes": [
				"Created before compose after durable records and live profile readback were clear."
			]
		})
	}

	fn valid_radar_archive_manifest() -> Value {
		serde_json::json!({
			"schema": "radar_archive_manifest/v1",
			"archive_id": "radar-archive-2026-06-02",
			"created_at": "2026-06-02T03:30:00Z",
			"retention_days": 21,
			"source_commit": "0123456789abcdef0123456789abcdef01234567",
			"release_tag": "radar-archive-2026-06-02",
			"release_url": "https://github.com/hack-ink/decodex/releases/tag/radar-archive-2026-06-02",
			"archive_asset": {
				"name": "radar-archive-2026-06-02.tar.zst",
				"size_bytes": 1_024,
				"sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
			},
			"checksum_asset": {
				"name": "SHA256SUMS",
				"sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
			},
			"files": [
				{
					"path": ".agent/automations/decodex/cache/github/bundles/openai-codex-pr-22414.json",
					"kind": "bundle",
					"sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
					"size_bytes": 512
				}
			]
		})
	}

	fn social_reserve_request(root: &Path, dry_run: bool) -> RadarSocialReservePublishRequest {
		RadarSocialReservePublishRequest {
			slug: "openai-codex-pr-22414".into(),
			mode: "operator_impact".into(),
			idempotency_key: "x:decodexspace:operator_impact:openai-codex-pr-22414".into(),
			reserved_at: "2026-06-02T02:50:00Z".into(),
			expires_at: "2026-06-02T03:50:00Z".into(),
			day: "2026-06-02".into(),
			timezone: "Asia/Shanghai".into(),
			candidate_paths: Vec::new(),
			urls: vec!["https://github.com/openai/codex/pull/22414".into()],
			duplicate_keys: vec![
				"Remote Codex can now use Unix socket endpoints.".into(),
				"https://github.com/openai/codex/pull/22414".into(),
			],
			out_dir: root.join("reservations"),
			posts_dir: root.join("posts"),
			automation_id: Some("decodex-x-publisher".into()),
			run_id: Some("2026-06-02T02:50:00Z".into()),
			branch: Some("xy/agent-home-cutover".into()),
			daily_limit: 8,
			dry_run,
		}
	}
}
