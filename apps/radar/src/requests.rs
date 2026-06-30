//! Public Radar command request and report contracts.

use std::{
	fmt::{self, Display, Formatter},
	path::PathBuf,
};

use serde::Serialize;

use super::{
	DEFAULT_LEDGER_PATH, DEFAULT_PAIR_LIMIT, DEFAULT_PREVIEW_LIMIT, DEFAULT_QUEUE_OUT,
	DEFAULT_RELEASE_DELTA_OUT, DEFAULT_SEARCH_LIMIT, DEFAULT_SIGNALS_DIR, DEFAULT_STABLE_LIMIT,
	DEFAULT_TAG_PREFIX,
};

/// Request to validate Radar JSON artifacts.
#[derive(Debug)]
pub(crate) struct RadarValidateRequest {
	/// Explicit files or directories to validate. Defaults to current checked Radar collections.
	pub(crate) paths: Vec<PathBuf>,
}

/// Request to refresh the deterministic upstream Radar review queue.
#[derive(Debug)]
pub(crate) struct RadarRefreshQueueRequest {
	/// GitHub repository in owner/name form.
	pub(crate) repo: String,
	/// How many recent upstream commits to inspect.
	pub(crate) search_limit: usize,
	/// Published signal directory used to suppress already-published subjects.
	pub(crate) signals_dir: PathBuf,
	/// Queue artifact output path.
	pub(crate) queue_out: PathBuf,
	/// Environment variable containing a GitHub token.
	pub(crate) token_env: Option<String>,
	/// Local Radar ledger path.
	pub(crate) ledger: PathBuf,
	/// Disable local Radar ledger writes.
	pub(crate) no_ledger: bool,
	/// Print the generated queue without writing the artifact.
	pub(crate) dry_run: bool,
}
impl Default for RadarRefreshQueueRequest {
	fn default() -> Self {
		Self {
			repo: "openai/codex".to_owned(),
			search_limit: DEFAULT_SEARCH_LIMIT,
			signals_dir: PathBuf::from(DEFAULT_SIGNALS_DIR),
			queue_out: PathBuf::from(DEFAULT_QUEUE_OUT),
			token_env: None,
			ledger: PathBuf::from(DEFAULT_LEDGER_PATH),
			no_ledger: false,
			dry_run: false,
		}
	}
}

/// Summary of an upstream Radar review queue refresh.
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct RadarRefreshQueueReport {
	/// Whether the checked-in queue artifact was rewritten.
	pub(crate) changed: bool,
	/// Number of recent commits scanned.
	pub(crate) recent_commits_scanned: usize,
	/// Number of scanned subjects already covered by published signals.
	pub(crate) published_subjects_seen: usize,
	/// Number of subjects queued for AI review.
	pub(crate) subjects_queued: usize,
	/// Whether local ledger writes were enabled.
	pub(crate) ledger_enabled: bool,
	/// Queue artifact path that was written or compared.
	pub(crate) queue_out: PathBuf,
}

/// Request to refresh the stable-versus-prerelease release-delta artifact.
#[derive(Debug)]
pub(crate) struct RadarRefreshReleaseDeltaRequest {
	/// GitHub repository in owner/name form.
	pub(crate) repo: String,
	/// Published signal directory used to map compare commits to signal slugs.
	pub(crate) signals_dir: PathBuf,
	/// Release-delta artifact output path.
	pub(crate) out: PathBuf,
	/// Release tag prefix to scope the tracked channel.
	pub(crate) tag_prefix: String,
	/// Environment variable containing a GitHub token.
	pub(crate) token_env: Option<String>,
	/// Maximum recent stable releases to include. Zero means all releases at or above the floor.
	pub(crate) stable_limit: usize,
	/// Maximum recent prereleases to include. Zero means all supported prereleases.
	pub(crate) preview_limit: usize,
	/// Maximum signal-bearing compare entries. Zero means all valid pairs.
	pub(crate) pair_limit: usize,
	/// Minimum stable tag included in comparator options.
	pub(crate) min_stable_tag: String,
	/// Print the generated release delta without writing the artifact.
	pub(crate) dry_run: bool,
}
impl Default for RadarRefreshReleaseDeltaRequest {
	fn default() -> Self {
		Self {
			repo: "openai/codex".to_owned(),
			signals_dir: PathBuf::from(DEFAULT_SIGNALS_DIR),
			out: PathBuf::from(DEFAULT_RELEASE_DELTA_OUT),
			tag_prefix: DEFAULT_TAG_PREFIX.to_owned(),
			token_env: None,
			stable_limit: DEFAULT_STABLE_LIMIT,
			preview_limit: DEFAULT_PREVIEW_LIMIT,
			pair_limit: DEFAULT_PAIR_LIMIT,
			min_stable_tag: super::DEFAULT_MIN_STABLE_TAG.to_owned(),
			dry_run: false,
		}
	}
}

/// Summary of a release-delta refresh.
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct RadarRefreshReleaseDeltaReport {
	/// Whether the checked-in release-delta artifact was rewritten.
	pub(crate) changed: bool,
	/// Stable release selected for the default comparison.
	pub(crate) stable_tag_name: String,
	/// Prerelease selected for the default comparison.
	pub(crate) prerelease_tag_name: String,
	/// Number of precomputed comparison entries.
	pub(crate) comparisons: usize,
	/// Release-delta artifact path that was written or compared.
	pub(crate) out: PathBuf,
}

/// Request to initialize the local Radar SQLite ledger.
#[derive(Debug)]
pub(crate) struct RadarLedgerBootstrapRequest {
	/// SQLite ledger path.
	pub(crate) db_path: PathBuf,
}

/// Request to ingest one bundle and optional derived artifacts into the Radar ledger.
#[derive(Debug)]
pub(crate) struct RadarLedgerIngestRequest {
	/// SQLite ledger path.
	pub(crate) db_path: PathBuf,
	/// Path to a `github_change_bundle/v1` JSON artifact.
	pub(crate) bundle_path: PathBuf,
	/// Optional analysis draft artifact path.
	pub(crate) analysis_path: Option<PathBuf>,
	/// Optional rendered `signal_entry/v1` artifact path.
	pub(crate) signal_path: Option<PathBuf>,
}

/// Request to ingest existing checked-in Radar artifacts into the Radar ledger.
#[derive(Debug)]
pub(crate) struct RadarLedgerIngestExistingRequest {
	/// SQLite ledger path.
	pub(crate) db_path: PathBuf,
	/// Directory containing `github_change_bundle/v1` JSON artifacts.
	pub(crate) bundles_dir: PathBuf,
	/// Directory containing analysis draft artifacts.
	pub(crate) analysis_dir: PathBuf,
	/// Directory containing rendered `signal_entry/v1` artifacts.
	pub(crate) signals_dir: PathBuf,
}

/// Request to attach one artifact path to an existing Radar subject.
#[derive(Debug)]
pub(crate) struct RadarLedgerArtifactLinkRequest {
	/// SQLite ledger path.
	pub(crate) db_path: PathBuf,
	/// GitHub repository in `owner/name` form.
	pub(crate) repo: String,
	/// Subject kind, either `commit` or `pr`.
	pub(crate) subject_kind: String,
	/// Subject id, either a commit SHA or pull request number.
	pub(crate) subject_id: String,
	/// Artifact kind stored in the ledger.
	pub(crate) artifact_kind: String,
	/// Artifact path to digest and link.
	pub(crate) path: PathBuf,
}

/// Request to summarize the local Radar SQLite ledger.
#[derive(Debug)]
pub(crate) struct RadarLedgerSummaryRequest {
	/// SQLite ledger path.
	pub(crate) db_path: PathBuf,
}

/// Request to build a deterministic GitHub change bundle.
#[derive(Debug)]
pub(crate) struct RadarBundleBuildRequest {
	/// GitHub repository in `owner/name` form.
	pub(crate) repo: String,
	/// Pull request number to fetch.
	pub(crate) pr: Option<u64>,
	/// Commit SHA to fetch when PR context is unavailable.
	pub(crate) commit: Option<String>,
	/// Skip commit-to-PR promotion when building from a commit.
	pub(crate) force_commit_only: bool,
	/// Optional environment variable name holding a GitHub token.
	pub(crate) token_env: Option<String>,
	/// Output path for the bundle JSON artifact.
	pub(crate) out: PathBuf,
	/// Additional note strings to store in the bundle.
	pub(crate) notes: Vec<String>,
}

/// Request to validate GitHub change bundle JSON artifacts.
#[derive(Debug)]
pub(crate) struct RadarBundleValidateRequest {
	/// Bundle JSON files or directories to validate.
	pub(crate) paths: Vec<PathBuf>,
}

/// Request to render one `signal_entry/v1` artifact from a bundle and analysis draft.
#[derive(Debug)]
pub(crate) struct RadarRenderSignalRequest {
	/// Path to a `github_change_bundle/v1` JSON artifact.
	pub(crate) bundle: PathBuf,
	/// Path to a Codex-owned `analysis_draft` JSON artifact.
	pub(crate) analysis: PathBuf,
	/// Path to write the rendered `signal_entry/v1` artifact.
	pub(crate) out: PathBuf,
	/// Optional publication timestamp override.
	pub(crate) published_at: Option<String>,
}

/// Summary of a rendered signal artifact.
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct RadarRenderSignalReport {
	/// Path that received the rendered signal artifact.
	pub(crate) out: PathBuf,
}

/// Request to backfill unpublished signals from a release-delta comparison window.
#[derive(Debug)]
pub(crate) struct RadarBackfillReleaseRangeRequest {
	/// GitHub repository in `owner/name` format.
	pub(crate) repo: String,
	/// Release-delta artifact to read or refresh.
	pub(crate) release_delta: PathBuf,
	/// Stable tag to use as the comparison start.
	pub(crate) stable_tag: Option<String>,
	/// Preview tag to use as the comparison end.
	pub(crate) preview_tag: Option<String>,
	/// Directory containing published signal entries.
	pub(crate) signals_dir: PathBuf,
	/// Directory for generated GitHub bundles.
	pub(crate) bundles_dir: PathBuf,
	/// Directory for Codex-owned analysis drafts.
	pub(crate) analysis_dir: PathBuf,
	/// Optional GitHub token environment variable name passed through to helper scripts.
	pub(crate) token_env: Option<String>,
	/// Codex executable to pass to the AI analysis boundary.
	pub(crate) codex_bin: String,
	/// Optional Codex model override for the AI analysis boundary.
	pub(crate) model: Option<String>,
	/// Optional PR count cap for partial runs.
	pub(crate) max_prs: Option<usize>,
	/// Print selected targets without writing generated content.
	pub(crate) dry_run: bool,
	/// Refresh the release-delta artifact into a temporary file before selecting targets.
	pub(crate) refresh_release_delta_first: bool,
	/// Stable release limit passed through only when refreshing first.
	pub(crate) refresh_stable_limit: Option<usize>,
	/// Preview release limit passed through only when refreshing first.
	pub(crate) refresh_preview_limit: Option<usize>,
	/// Compare-pair limit passed through only when refreshing first.
	pub(crate) refresh_pair_limit: Option<usize>,
	/// Python executable used for the Codex AI analysis helper boundary.
	pub(crate) python_bin: String,
}

/// Summary of a release-window backfill selection or run.
#[derive(Debug, Eq, PartialEq, Serialize)]
pub(crate) struct RadarBackfillReleaseRangeReport {
	/// Stable tag selected for the comparison.
	pub(crate) stable_tag: String,
	/// Preview tag selected for the comparison.
	pub(crate) preview_tag: String,
	/// PR numbers selected for backfill.
	pub(crate) target_prs: Vec<u64>,
	/// Number of signal entries created by this run.
	pub(crate) created: usize,
	/// Whether the command only previewed targets.
	pub(crate) dry_run: bool,
}
impl Display for RadarBackfillReleaseRangeReport {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
		write!(formatter, "{}", serde_json::to_string_pretty(self).map_err(|_| fmt::Error)?)
	}
}

/// Summary of a Radar validation pass.
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct RadarValidationReport {
	/// Number of JSON files parsed and validated.
	pub(crate) checked_files: usize,
}
