use std::path::PathBuf;

use serde::Serialize;

#[derive(Debug, Serialize)]
pub(crate) struct SocialBrowserLeaseReport {
	pub(crate) status: String,
	pub(crate) path: String,
	pub(crate) lease_token: Option<String>,
	pub(crate) expires_at_epoch_seconds: Option<u64>,
}

#[derive(Debug)]
pub(crate) struct SocialReservePublishRequest {
	pub(crate) slug: String,
	pub(crate) mode: String,
	pub(crate) idempotency_key: String,
	pub(crate) reserved_at: String,
	pub(crate) expires_at: String,
	pub(crate) day: String,
	pub(crate) timezone: String,
	pub(crate) candidate_paths: Vec<PathBuf>,
	pub(crate) urls: Vec<String>,
	pub(crate) duplicate_keys: Vec<String>,
	pub(crate) out_dir: PathBuf,
	pub(crate) posts_dir: PathBuf,
	pub(crate) locks_dir: PathBuf,
	pub(crate) browser_lease_token: String,
	pub(crate) automation_id: Option<String>,
	pub(crate) run_id: Option<String>,
	pub(crate) branch: Option<String>,
	pub(crate) daily_limit: usize,
	pub(crate) dry_run: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct SocialReservePublishReport {
	pub(crate) status: String,
	pub(crate) path: String,
	pub(crate) idempotency_key: String,
	pub(crate) daily_limit: usize,
	pub(crate) published_count: usize,
	pub(crate) active_reservation_count: usize,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct SocialValidationReport {
	pub(crate) checked_files: usize,
	pub(crate) errors: Vec<String>,
}
