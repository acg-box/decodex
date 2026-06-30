use std::time::Duration;

pub(super) const DEFAULT_LOG_ROTATE_BYTES: u64 = 10 * 1_024 * 1_024;
pub(super) const DEFAULT_LOG_RETENTION_DAYS: u64 = 14;
pub(super) const DEFAULT_EVIDENCE_ROTATE_BYTES: u64 = 10 * 1_024 * 1_024;
pub(super) const DEFAULT_EVIDENCE_RETENTION_DAYS: u64 = 14;
pub(super) const DEFAULT_PROTOCOL_EVENT_RETENTION_DAYS: i64 = 14;
pub(super) const DEFAULT_GIT_ASKPASS_HELPER_RETENTION_DAYS: u64 = 1;
pub(super) const DEFAULT_BACKUP_KEEP_RECENT: usize = 3;
pub(super) const DEFAULT_BACKUP_RETENTION_DAYS: u64 = 7;
pub(super) const LEGACY_GIT_ASKPASS_PREFIX: &str = ".decodex-git-askpass-";
pub(super) const LEGACY_GIT_ASKPASS_SUFFIX: &str = ".sh";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MaintenanceMode {
	DryRun,
	Apply,
}
impl MaintenanceMode {
	pub(super) fn as_str(self) -> &'static str {
		match self {
			Self::DryRun => "dry-run",
			Self::Apply => "apply",
		}
	}

	pub(super) fn applies(self) -> bool {
		matches!(self, Self::Apply)
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MaintenanceScope {
	Full,
	AutoSafe,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct MaintenancePruneRequest {
	pub(crate) mode: MaintenanceMode,
	pub(crate) scope: MaintenanceScope,
	pub(crate) json: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct MaintenancePolicy {
	pub(crate) log_rotate_bytes: u64,
	pub(crate) log_retention: Duration,
	pub(crate) evidence_rotate_bytes: u64,
	pub(crate) evidence_retention: Duration,
	pub(crate) protocol_event_retention_days: i64,
	pub(crate) git_askpass_helper_retention: Duration,
	pub(crate) backup_keep_recent: usize,
	pub(crate) backup_retention: Duration,
}
impl MaintenancePolicy {
	pub(crate) fn default() -> Self {
		Self {
			log_rotate_bytes: DEFAULT_LOG_ROTATE_BYTES,
			log_retention: Duration::from_secs(DEFAULT_LOG_RETENTION_DAYS * 24 * 60 * 60),
			evidence_rotate_bytes: DEFAULT_EVIDENCE_ROTATE_BYTES,
			evidence_retention: Duration::from_secs(DEFAULT_EVIDENCE_RETENTION_DAYS * 24 * 60 * 60),
			protocol_event_retention_days: DEFAULT_PROTOCOL_EVENT_RETENTION_DAYS,
			git_askpass_helper_retention: Duration::from_secs(
				DEFAULT_GIT_ASKPASS_HELPER_RETENTION_DAYS * 24 * 60 * 60,
			),
			backup_keep_recent: DEFAULT_BACKUP_KEEP_RECENT,
			backup_retention: Duration::from_secs(DEFAULT_BACKUP_RETENTION_DAYS * 24 * 60 * 60),
		}
	}
}
