use std::path::PathBuf;

use serde::Serialize;

use crate::{DEFAULT_LEDGER_PATH, DEFAULT_QUEUE_OUT, DEFAULT_SEARCH_LIMIT, DEFAULT_SIGNALS_DIR};

/// Request to validate Radar JSON artifacts.
#[derive(Debug)]
pub(crate) struct RadarValidateRequest {
	/// Explicit files or directories to validate. Defaults to current checked Radar collections.
	pub(crate) paths: Vec<PathBuf>,
	/// Maximum source snapshot age. Default collection validation uses the Radar default.
	pub(crate) max_age_hours: Option<u64>,
	/// Permit an empty default cache only during an explicit first-run bootstrap.
	pub(crate) bootstrap: bool,
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
#[derive(Debug, Eq, PartialEq, Serialize)]
pub(crate) struct RadarRefreshQueueReport {
	/// Whether source identity or content changed.
	pub(crate) material_changed: bool,
	/// Whether this run refreshed the artifact on disk.
	pub(crate) written: bool,
	/// Timestamp carried by the successful observation.
	pub(crate) refreshed_at: String,
	/// SHA-256 of the exact canonical queue bytes produced by this refresh.
	pub(crate) queue_sha256: String,
	/// Number of recent commits scanned.
	pub(crate) recent_commits_scanned: usize,
	/// Number of scanned subjects already covered by published signals.
	pub(crate) published_subjects_seen: usize,
	/// Number of subjects queued for AI review.
	pub(crate) subjects_queued: usize,
	/// Whether local ledger writes were enabled.
	pub(crate) ledger_enabled: bool,
}

/// Summary of a Radar validation pass.
#[derive(Debug, Eq, PartialEq, Serialize)]
pub(crate) struct RadarValidationReport {
	/// Number of JSON files parsed and validated.
	pub(crate) checked_files: usize,
	/// Bounded retention evidence for default daily validation.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) cache_gc: Option<crate::RadarCacheGcReport>,
}
