use std::{
	fmt::{self, Display, Formatter},
	path::PathBuf,
};

use serde::Serialize;

use crate::{
	DEFAULT_MIN_STABLE_TAG, DEFAULT_PAIR_LIMIT, DEFAULT_PREVIEW_LIMIT, DEFAULT_RELEASE_DELTA_OUT,
	DEFAULT_SIGNALS_DIR, DEFAULT_STABLE_LIMIT, DEFAULT_TAG_PREFIX,
};

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
			min_stable_tag: DEFAULT_MIN_STABLE_TAG.to_owned(),
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
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		write!(formatter, "{}", serde_json::to_string_pretty(self).map_err(|_| fmt::Error)?)
	}
}
