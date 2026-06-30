use std::{path::PathBuf, time::SystemTime};

use serde::Serialize;

#[derive(Debug, Serialize)]
pub(crate) struct MaintenanceReport {
	pub(crate) schema: &'static str,
	pub(crate) mode: String,
	pub(crate) scope: String,
	pub(crate) generated_at: String,
	pub(crate) logs: FileMaintenanceReport,
	pub(crate) agent_evidence: FileMaintenanceReport,
	pub(crate) git_askpass_helpers: FileMaintenanceReport,
	pub(crate) backups: BackupMaintenanceReport,
	pub(crate) runtime: RuntimeMaintenanceReport,
	pub(crate) wal_checkpoint: Option<WalCheckpointReport>,
}

#[derive(Debug, Default, Serialize)]
pub(crate) struct FileMaintenanceReport {
	pub(crate) root: String,
	pub(crate) rotate_candidates: usize,
	pub(crate) rotated_files: usize,
	pub(crate) rotate_bytes: u64,
	pub(crate) delete_candidates: usize,
	pub(crate) deleted_files: usize,
	pub(crate) delete_bytes: u64,
	pub(crate) actions: Vec<FileMaintenanceAction>,
}

#[derive(Debug, Default, Serialize)]
pub(crate) struct BackupMaintenanceReport {
	pub(crate) root: String,
	pub(crate) delete_candidates: usize,
	pub(crate) deleted_files: usize,
	pub(crate) delete_bytes: u64,
	pub(crate) actions: Vec<FileMaintenanceAction>,
}

#[derive(Debug, Default, Serialize)]
pub(crate) struct RuntimeMaintenanceReport {
	pub(crate) database_path: String,
	pub(crate) protocol_event_retention_days: i64,
	pub(crate) protected_run_count: usize,
	pub(crate) protocol_run_candidates: usize,
	pub(crate) protocol_event_candidates: u64,
	pub(crate) compacted_runs: usize,
	pub(crate) compacted_events: u64,
	pub(crate) actions: Vec<RuntimeMaintenanceAction>,
	pub(crate) warnings: Vec<RuntimeMaintenanceWarning>,
}

#[derive(Debug, Serialize)]
pub(crate) struct WalCheckpointReport {
	pub(crate) mode: &'static str,
	pub(crate) busy: i64,
	pub(crate) log_frames: i64,
	pub(crate) checkpointed_frames: i64,
}

#[derive(Debug, Serialize)]
pub(crate) struct RuntimeMaintenanceWarning {
	pub(crate) warning: &'static str,
	pub(crate) reason: &'static str,
}

#[derive(Debug, Serialize)]
pub(crate) struct FileMaintenanceAction {
	pub(crate) action: &'static str,
	pub(crate) path: String,
	pub(crate) bytes: u64,
	pub(crate) target: Option<String>,
	pub(crate) reason: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct RuntimeMaintenanceAction {
	pub(crate) action: &'static str,
	pub(crate) run_id: String,
	pub(crate) issue_id: String,
	pub(crate) status: String,
	pub(crate) event_count: u64,
	pub(crate) last_event_at: Option<String>,
	pub(crate) reason: String,
}

pub(crate) struct RuntimeProtocolCandidate {
	pub(crate) run_id: String,
	pub(crate) issue_id: String,
	pub(crate) status: String,
	pub(crate) event_count: u64,
	pub(crate) last_sequence_number: Option<i64>,
	pub(crate) last_event_type: Option<String>,
	pub(crate) last_event_at: Option<String>,
	pub(crate) last_event_at_unix: Option<i64>,
}

#[derive(Clone)]
pub(crate) struct BackupCandidate {
	pub(crate) path: PathBuf,
	pub(crate) bytes: u64,
	pub(crate) modified: SystemTime,
}
