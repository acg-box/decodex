use std::{
	cmp::Reverse,
	collections::BTreeMap,
	fs::{self, Metadata, OpenOptions},
	io::{self, Write as _},
	path::{Path, PathBuf},
	time::{Duration, SystemTime},
};

use color_eyre::Report;
use rusqlite::{self, Connection};
use serde::Serialize;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	prelude::{Result, eyre},
	runtime,
};

const DEFAULT_LOG_ROTATE_BYTES: u64 = 10 * 1_024 * 1_024;
const DEFAULT_LOG_RETENTION_DAYS: u64 = 14;
const DEFAULT_EVIDENCE_ROTATE_BYTES: u64 = 10 * 1_024 * 1_024;
const DEFAULT_EVIDENCE_RETENTION_DAYS: u64 = 14;
const DEFAULT_PROTOCOL_EVENT_RETENTION_DAYS: i64 = 14;
const DEFAULT_GIT_ASKPASS_HELPER_RETENTION_DAYS: u64 = 1;
const DEFAULT_BACKUP_KEEP_RECENT: usize = 3;
const DEFAULT_BACKUP_RETENTION_DAYS: u64 = 7;
const LEGACY_GIT_ASKPASS_PREFIX: &str = ".decodex-git-askpass-";
const LEGACY_GIT_ASKPASS_SUFFIX: &str = ".sh";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MaintenanceMode {
	DryRun,
	Apply,
}
impl MaintenanceMode {
	fn as_str(self) -> &'static str {
		match self {
			Self::DryRun => "dry-run",
			Self::Apply => "apply",
		}
	}

	fn applies(self) -> bool {
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

#[derive(Debug, Serialize)]
pub(crate) struct MaintenanceReport {
	schema: &'static str,
	mode: String,
	scope: String,
	generated_at: String,
	pub(crate) logs: FileMaintenanceReport,
	pub(crate) agent_evidence: FileMaintenanceReport,
	pub(crate) git_askpass_helpers: FileMaintenanceReport,
	pub(crate) backups: BackupMaintenanceReport,
	pub(crate) runtime: RuntimeMaintenanceReport,
	pub(crate) wal_checkpoint: Option<WalCheckpointReport>,
}

#[derive(Debug, Default, Serialize)]
pub(crate) struct FileMaintenanceReport {
	root: String,
	rotate_candidates: usize,
	pub(crate) rotated_files: usize,
	rotate_bytes: u64,
	delete_candidates: usize,
	pub(crate) deleted_files: usize,
	delete_bytes: u64,
	actions: Vec<FileMaintenanceAction>,
}

#[derive(Debug, Default, Serialize)]
pub(crate) struct BackupMaintenanceReport {
	root: String,
	delete_candidates: usize,
	pub(crate) deleted_files: usize,
	delete_bytes: u64,
	actions: Vec<FileMaintenanceAction>,
}

#[derive(Debug, Default, Serialize)]
pub(crate) struct RuntimeMaintenanceReport {
	database_path: String,
	protocol_event_retention_days: i64,
	protected_run_count: usize,
	protocol_run_candidates: usize,
	protocol_event_candidates: u64,
	compacted_runs: usize,
	compacted_events: u64,
	actions: Vec<RuntimeMaintenanceAction>,
	warnings: Vec<RuntimeMaintenanceWarning>,
}

#[derive(Debug, Serialize)]
pub(crate) struct WalCheckpointReport {
	pub(crate) mode: &'static str,
	busy: i64,
	log_frames: i64,
	checkpointed_frames: i64,
}

#[derive(Debug, Serialize)]
pub(crate) struct RuntimeMaintenanceWarning {
	warning: &'static str,
	reason: &'static str,
}

#[derive(Clone, Copy)]
struct MaintenancePolicy {
	log_rotate_bytes: u64,
	log_retention: Duration,
	evidence_rotate_bytes: u64,
	evidence_retention: Duration,
	protocol_event_retention_days: i64,
	git_askpass_helper_retention: Duration,
	backup_keep_recent: usize,
	backup_retention: Duration,
}
impl MaintenancePolicy {
	fn default() -> Self {
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

#[derive(Debug, Serialize)]
struct FileMaintenanceAction {
	action: &'static str,
	path: String,
	bytes: u64,
	target: Option<String>,
	reason: String,
}

#[derive(Debug, Serialize)]
struct RuntimeMaintenanceAction {
	action: &'static str,
	run_id: String,
	issue_id: String,
	status: String,
	event_count: u64,
	last_event_at: Option<String>,
	reason: String,
}

struct RuntimeProtocolCandidate {
	run_id: String,
	issue_id: String,
	status: String,
	event_count: u64,
	last_sequence_number: Option<i64>,
	last_event_type: Option<String>,
	last_event_at: Option<String>,
	last_event_at_unix: Option<i64>,
}

#[derive(Clone)]
struct BackupCandidate {
	path: PathBuf,
	bytes: u64,
	modified: SystemTime,
}

pub(crate) fn run_prune_command(request: MaintenancePruneRequest) -> Result<()> {
	let report = run_prune_with_policy(request, MaintenancePolicy::default())?;

	if request.json {
		println!("{}", serde_json::to_string_pretty(&report)?);
	} else {
		print_prune_report(&report)?;
	}

	Ok(())
}

pub(crate) fn run_auto_safe_prune() -> Result<MaintenanceReport> {
	run_prune_with_policy(
		MaintenancePruneRequest {
			mode: MaintenanceMode::Apply,
			scope: MaintenanceScope::AutoSafe,
			json: false,
		},
		MaintenancePolicy::default(),
	)
}

pub(crate) fn ensure_protocol_event_summary_table(connection: &Connection) -> Result<()> {
	connection.execute_batch(
		"CREATE TABLE IF NOT EXISTS protocol_event_summaries (
			run_id TEXT PRIMARY KEY NOT NULL,
			event_count INTEGER NOT NULL,
			last_sequence_number INTEGER,
			last_event_type TEXT,
			last_event_at TEXT,
			last_event_at_unix INTEGER,
			compacted_at TEXT NOT NULL,
			compacted_at_unix INTEGER NOT NULL
		);",
	)?;

	Ok(())
}

fn run_prune_with_policy(
	request: MaintenancePruneRequest,
	policy: MaintenancePolicy,
) -> Result<MaintenanceReport> {
	let generated_at = OffsetDateTime::now_utc();
	let system_now = SystemTime::now();
	let logs = maintain_logs(request.mode, policy, system_now, generated_at)?;
	let agent_evidence = maintain_agent_evidence(request.mode, policy, system_now, generated_at)?;
	let git_askpass_helpers =
		maintain_git_askpass_helpers_for_scope(request.mode, request.scope, policy, system_now)?;
	let backups = maintain_backups(request.mode, policy, system_now)?;
	let runtime = maintain_runtime(request.mode, request.scope, policy, generated_at)?;
	let wal_checkpoint = maintain_wal(request.mode, request.scope)?;

	Ok(MaintenanceReport {
		schema: "decodex.maintenance_report/1",
		mode: request.mode.as_str().to_owned(),
		scope: match request.scope {
			MaintenanceScope::Full => String::from("full"),
			MaintenanceScope::AutoSafe => String::from("auto-safe"),
		},
		generated_at: generated_at.format(&Rfc3339)?,
		logs,
		agent_evidence,
		git_askpass_helpers,
		backups,
		runtime,
		wal_checkpoint,
	})
}

fn maintain_logs(
	mode: MaintenanceMode,
	policy: MaintenancePolicy,
	system_now: SystemTime,
	generated_at: OffsetDateTime,
) -> Result<FileMaintenanceReport> {
	let root = runtime::log_dir()?;
	let mut report = FileMaintenanceReport {
		root: root.display().to_string(),
		..FileMaintenanceReport::default()
	};

	if !root.exists() {
		return Ok(report);
	}

	for entry in fs::read_dir(&root)? {
		let entry = entry?;
		let path = entry.path();

		if !path.is_file()
			|| path.extension().and_then(|extension| extension.to_str()) != Some("log")
		{
			continue;
		}

		let metadata = entry.metadata()?;
		let size = metadata.len();

		if is_rotated_log_file(&path)
			&& file_is_older_than(&metadata, system_now, policy.log_retention)
		{
			report.delete_candidates += 1;
			report.delete_bytes = report.delete_bytes.saturating_add(size);

			report.actions.push(FileMaintenanceAction {
				action: "delete",
				path: path.display().to_string(),
				bytes: size,
				target: None,
				reason: format!("log is older than {DEFAULT_LOG_RETENTION_DAYS} days"),
			});

			if mode.applies() {
				fs::remove_file(&path)?;

				report.deleted_files += 1;
			}

			continue;
		}
		if size > policy.log_rotate_bytes {
			let rotated_path = rotated_path(&path, generated_at)?;

			report.rotate_candidates += 1;
			report.rotate_bytes = report.rotate_bytes.saturating_add(size);

			report.actions.push(FileMaintenanceAction {
				action: "rotate",
				path: path.display().to_string(),
				bytes: size,
				target: Some(rotated_path.display().to_string()),
				reason: format!(
					"size {} exceeds {} byte log rotation threshold",
					size, policy.log_rotate_bytes
				),
			});

			if mode.applies() {
				copy_truncate(&path, &rotated_path)?;

				report.rotated_files += 1;
			}
		}
	}

	Ok(report)
}

fn maintain_agent_evidence(
	mode: MaintenanceMode,
	policy: MaintenancePolicy,
	system_now: SystemTime,
	generated_at: OffsetDateTime,
) -> Result<FileMaintenanceReport> {
	let root = runtime::agent_evidence_dir()?;
	let mut report = FileMaintenanceReport {
		root: root.display().to_string(),
		..FileMaintenanceReport::default()
	};

	if !root.exists() {
		return Ok(report);
	}

	for entry in fs::read_dir(&root)? {
		let entry = entry?;
		let service_root = entry.path();

		if !service_root.is_dir() {
			continue;
		}

		let events_path = service_root.join("events.jsonl");

		if events_path.is_file() {
			let metadata = fs::metadata(&events_path)?;
			let size = metadata.len();

			if size > policy.evidence_rotate_bytes {
				let rotated_path = rotated_path(&events_path, generated_at)?;

				report.rotate_candidates += 1;
				report.rotate_bytes = report.rotate_bytes.saturating_add(size);

				report.actions.push(FileMaintenanceAction {
					action: "rotate",
					path: events_path.display().to_string(),
					bytes: size,
					target: Some(rotated_path.display().to_string()),
					reason: format!(
						"size {} exceeds {} byte agent-evidence event threshold",
						size, policy.evidence_rotate_bytes
					),
				});

				if mode.applies() {
					copy_truncate(&events_path, &rotated_path)?;

					report.rotated_files += 1;
				}
			}
		}

		for event_entry in fs::read_dir(&service_root)? {
			let event_entry = event_entry?;
			let path = event_entry.path();
			let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
				continue;
			};

			if !path.is_file()
				|| file_name == "events.jsonl"
				|| !file_name.starts_with("events.")
				|| path.extension().and_then(|extension| extension.to_str()) != Some("jsonl")
			{
				continue;
			}

			let metadata = event_entry.metadata()?;

			if file_is_older_than(&metadata, system_now, policy.evidence_retention) {
				let size = metadata.len();

				report.delete_candidates += 1;
				report.delete_bytes = report.delete_bytes.saturating_add(size);

				report.actions.push(FileMaintenanceAction {
					action: "delete",
					path: path.display().to_string(),
					bytes: size,
					target: None,
					reason: format!(
						"rotated agent-evidence event stream is older than {DEFAULT_EVIDENCE_RETENTION_DAYS} days"
					),
				});

				if mode.applies() {
					fs::remove_file(&path)?;

					report.deleted_files += 1;
				}
			}
		}
	}

	Ok(report)
}

fn maintain_git_askpass_helpers_for_scope(
	mode: MaintenanceMode,
	scope: MaintenanceScope,
	policy: MaintenancePolicy,
	system_now: SystemTime,
) -> Result<FileMaintenanceReport> {
	match maintain_git_askpass_helpers(mode, policy, system_now) {
		Ok(report) => Ok(report),
		Err(error) if scope == MaintenanceScope::AutoSafe => {
			tracing::warn!(
				?error,
				"Skipped Decodex auto-safe legacy Git askpass helper cleanup; control-plane maintenance continued."
			);

			Ok(FileMaintenanceReport {
				root: runtime::runtime_db_path()?.display().to_string(),
				..FileMaintenanceReport::default()
			})
		},
		Err(error) => Err(error),
	}
}

fn maintain_git_askpass_helpers(
	mode: MaintenanceMode,
	policy: MaintenancePolicy,
	system_now: SystemTime,
) -> Result<FileMaintenanceReport> {
	let database_path = runtime::runtime_db_path()?;
	let mut report = FileMaintenanceReport {
		root: database_path.display().to_string(),
		..FileMaintenanceReport::default()
	};

	if !database_path.exists() {
		return Ok(report);
	}

	let connection = Connection::open(database_path)?;

	if !sqlite_table_exists(&connection, "projects")? {
		return Ok(report);
	}

	for worktree_root in registered_worktree_roots(&connection)? {
		if !worktree_root.exists() {
			continue;
		}

		for entry in fs::read_dir(&worktree_root)? {
			let entry = entry?;
			let path = entry.path();
			let file_type = entry.file_type()?;

			if !file_type.is_file() || !is_legacy_git_askpass_helper(&path) {
				continue;
			}

			let metadata = entry.metadata()?;

			if !file_is_older_than(&metadata, system_now, policy.git_askpass_helper_retention) {
				continue;
			}

			let size = metadata.len();

			report.delete_candidates += 1;
			report.delete_bytes = report.delete_bytes.saturating_add(size);

			report.actions.push(FileMaintenanceAction {
				action: "delete",
				path: path.display().to_string(),
				bytes: size,
				target: None,
				reason: format!(
					"legacy Git askpass helper is older than {DEFAULT_GIT_ASKPASS_HELPER_RETENTION_DAYS} day"
				),
			});

			if mode.applies() {
				fs::remove_file(&path)?;

				report.deleted_files += 1;
			}
		}
	}

	Ok(report)
}

fn maintain_backups(
	mode: MaintenanceMode,
	policy: MaintenancePolicy,
	system_now: SystemTime,
) -> Result<BackupMaintenanceReport> {
	let root = runtime::decodex_home_dir()?;
	let mut report = BackupMaintenanceReport {
		root: root.display().to_string(),
		..BackupMaintenanceReport::default()
	};

	if !root.exists() {
		return Ok(report);
	}

	let mut groups = BTreeMap::<String, Vec<BackupCandidate>>::new();

	collect_backup_candidates(&root, &mut groups)?;

	for candidates in groups.values_mut() {
		candidates.sort_by_key(|candidate| Reverse(candidate.modified));

		for (index, candidate) in candidates.iter().enumerate() {
			let young_enough = system_now
				.duration_since(candidate.modified)
				.map(|age| age <= policy.backup_retention)
				.unwrap_or(true);
			let recent_enough = index < policy.backup_keep_recent;

			if recent_enough || young_enough {
				continue;
			}

			report.delete_candidates += 1;
			report.delete_bytes = report.delete_bytes.saturating_add(candidate.bytes);

			report.actions.push(FileMaintenanceAction {
				action: "delete",
				path: candidate.path.display().to_string(),
				bytes: candidate.bytes,
				target: None,
				reason: format!(
					"backup is outside the latest {} files and older than {DEFAULT_BACKUP_RETENTION_DAYS} days",
					policy.backup_keep_recent
				),
			});

			if mode.applies() {
				fs::remove_file(&candidate.path)?;

				report.deleted_files += 1;
			}
		}
	}

	Ok(report)
}

fn maintain_runtime(
	mode: MaintenanceMode,
	scope: MaintenanceScope,
	policy: MaintenancePolicy,
	generated_at: OffsetDateTime,
) -> Result<RuntimeMaintenanceReport> {
	let database_path = runtime::runtime_db_path()?;
	let mut report = RuntimeMaintenanceReport {
		database_path: database_path.display().to_string(),
		protocol_event_retention_days: policy.protocol_event_retention_days,
		..RuntimeMaintenanceReport::default()
	};

	if !database_path.exists() {
		return Ok(report);
	}

	let runtime_result =
		maintain_runtime_protocol_events(&database_path, &mut report, mode, policy, generated_at);

	match runtime_result {
		Ok(()) => Ok(report),
		Err(error) if scope == MaintenanceScope::AutoSafe => {
			let warning = runtime_maintenance_warning_for_error(&error);

			tracing::warn!(
				warning = warning.warning,
				reason = warning.reason,
				"Skipped Decodex auto-safe protocol-event compaction; control-plane maintenance continued."
			);

			report.warnings.push(warning);

			Ok(report)
		},
		Err(error) => Err(error),
	}
}

fn maintain_runtime_protocol_events(
	database_path: &Path,
	report: &mut RuntimeMaintenanceReport,
	mode: MaintenanceMode,
	policy: MaintenancePolicy,
	generated_at: OffsetDateTime,
) -> Result<()> {
	let mut connection = Connection::open(database_path)?;

	connection.busy_timeout(Duration::from_secs(5))?;

	if mode.applies() {
		ensure_protocol_event_summary_table(&connection)?;
	}

	let cutoff_unix =
		generated_at.unix_timestamp() - policy.protocol_event_retention_days * 24 * 60 * 60;
	let candidates = protocol_event_compaction_candidates(&connection, cutoff_unix)?;

	report.protocol_run_candidates = candidates.len();
	report.protocol_event_candidates =
		candidates.iter().map(|candidate| candidate.event_count).sum::<u64>();
	report.protected_run_count = protected_protocol_run_count(&connection)?;

	for candidate in &candidates {
		report.actions.push(RuntimeMaintenanceAction {
			action: "compact-protocol-events",
			run_id: candidate.run_id.clone(),
			issue_id: candidate.issue_id.clone(),
			status: candidate.status.clone(),
			event_count: candidate.event_count,
			last_event_at: candidate.last_event_at.clone(),
			reason: format!(
				"terminal run has no run lease, retained worktree, or review lifecycle record and its latest protocol event is older than {} days",
				policy.protocol_event_retention_days
			),
		});
	}

	if mode.applies() && !candidates.is_empty() {
		compact_protocol_events(&mut connection, &candidates, generated_at)?;

		report.compacted_runs = candidates.len();
		report.compacted_events = report.protocol_event_candidates;
	}

	Ok(())
}

fn maintain_wal(
	mode: MaintenanceMode,
	scope: MaintenanceScope,
) -> Result<Option<WalCheckpointReport>> {
	if mode == MaintenanceMode::DryRun {
		return Ok(None);
	}

	let database_path = runtime::runtime_db_path()?;

	if !database_path.exists() {
		return Ok(None);
	}

	let connection = Connection::open(database_path)?;
	let checkpoint_mode = match scope {
		MaintenanceScope::Full => "TRUNCATE",
		MaintenanceScope::AutoSafe => "PASSIVE",
	};
	let mut statement = connection.prepare(&format!("PRAGMA wal_checkpoint({checkpoint_mode})"))?;
	let report = statement.query_row([], |row| {
		Ok(WalCheckpointReport {
			mode: checkpoint_mode,
			busy: row.get(0)?,
			log_frames: row.get(1)?,
			checkpointed_frames: row.get(2)?,
		})
	})?;

	Ok(Some(report))
}

fn protocol_event_compaction_candidates(
	connection: &Connection,
	cutoff_unix: i64,
) -> Result<Vec<RuntimeProtocolCandidate>> {
	let mut statement = connection.prepare(
		"SELECT
			attempts.run_id,
			attempts.issue_id,
			attempts.status,
			totals.event_count,
			totals.last_sequence_number,
			last.event_type,
			last.created_at,
			last.created_at_unix
		 FROM (
			SELECT
				run_id,
				COUNT(*) AS event_count,
				MAX(sequence_number) AS last_sequence_number,
				MAX(created_at_unix) AS last_created_at_unix
			FROM protocol_events
			GROUP BY run_id
		 ) totals
		 JOIN run_attempts attempts ON attempts.run_id = totals.run_id
		 JOIN protocol_events last
			ON last.run_id = totals.run_id
			AND last.sequence_number = totals.last_sequence_number
		 LEFT JOIN leases run_lease ON run_lease.issue_id = attempts.issue_id
		 LEFT JOIN worktrees retained_worktree ON retained_worktree.issue_id = attempts.issue_id
		 LEFT JOIN review_lifecycle_records review_lifecycle
			ON review_lifecycle.issue_id = attempts.issue_id
		 LEFT JOIN (
			SELECT
				issue_id,
				json_extract(payload_json, '$.run_id') AS run_id
			FROM linear_execution_events
			WHERE event_type IN ('needs_attention', 'terminal_failure')
				AND json_valid(payload_json)
		 ) human_stop_event
			ON human_stop_event.issue_id = attempts.issue_id
			AND human_stop_event.run_id = attempts.run_id
		 WHERE attempts.status IN ('succeeded', 'failed', 'interrupted', 'terminated')
			AND totals.last_created_at_unix < ?1
			AND run_lease.issue_id IS NULL
			AND retained_worktree.issue_id IS NULL
			AND review_lifecycle.issue_id IS NULL
			AND human_stop_event.run_id IS NULL
		 ORDER BY totals.last_created_at_unix ASC, attempts.run_id ASC",
	)?;
	let rows = statement.query_map(rusqlite::params![cutoff_unix], |row| {
		Ok(RuntimeProtocolCandidate {
			run_id: row.get(0)?,
			issue_id: row.get(1)?,
			status: row.get(2)?,
			event_count: row.get::<_, i64>(3).map(|value| value.max(0) as u64)?,
			last_sequence_number: row.get(4)?,
			last_event_type: row.get(5)?,
			last_event_at: row.get(6)?,
			last_event_at_unix: row.get(7)?,
		})
	})?;
	let mut candidates = Vec::new();

	for row in rows {
		candidates.push(row?);
	}

	Ok(candidates)
}

fn protected_protocol_run_count(connection: &Connection) -> Result<usize> {
	let count = connection.query_row(
		"SELECT COUNT(DISTINCT attempts.run_id)
		 FROM run_attempts attempts
		 JOIN protocol_events events ON events.run_id = attempts.run_id
		 LEFT JOIN leases run_lease ON run_lease.issue_id = attempts.issue_id
		 LEFT JOIN worktrees retained_worktree ON retained_worktree.issue_id = attempts.issue_id
		 LEFT JOIN review_lifecycle_records review_lifecycle
			ON review_lifecycle.issue_id = attempts.issue_id
		 LEFT JOIN (
			SELECT
				issue_id,
				json_extract(payload_json, '$.run_id') AS run_id
			FROM linear_execution_events
			WHERE event_type IN ('needs_attention', 'terminal_failure')
				AND json_valid(payload_json)
		 ) human_stop_event
			ON human_stop_event.issue_id = attempts.issue_id
			AND human_stop_event.run_id = attempts.run_id
		 WHERE run_lease.issue_id IS NOT NULL
			OR retained_worktree.issue_id IS NOT NULL
			OR review_lifecycle.issue_id IS NOT NULL
			OR human_stop_event.run_id IS NOT NULL
			OR attempts.status NOT IN ('succeeded', 'failed', 'interrupted', 'terminated')",
		[],
		|row| row.get::<_, i64>(0),
	)?;

	Ok(count.max(0) as usize)
}

fn runtime_maintenance_warning_for_error(error: &Report) -> RuntimeMaintenanceWarning {
	let message = error.to_string().to_ascii_lowercase();
	let reason =
		if message.contains("busy") || message.contains("locked") || message.contains("sqlite") {
			"sqlite_unavailable"
		} else {
			"candidate_detection_failed"
		};

	RuntimeMaintenanceWarning { warning: "auto_protocol_event_compaction_skipped", reason }
}

fn registered_worktree_roots(connection: &Connection) -> Result<Vec<PathBuf>> {
	let mut statement =
		connection.prepare("SELECT DISTINCT worktree_root FROM projects ORDER BY worktree_root")?;
	let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
	let mut roots = Vec::new();

	for row in rows {
		roots.push(PathBuf::from(row?));
	}

	Ok(roots)
}

fn sqlite_table_exists(connection: &Connection, table: &str) -> Result<bool> {
	let count = connection.query_row(
		"SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
		rusqlite::params![table],
		|row| row.get::<_, i64>(0),
	)?;

	Ok(count > 0)
}

fn compact_protocol_events(
	connection: &mut Connection,
	candidates: &[RuntimeProtocolCandidate],
	generated_at: OffsetDateTime,
) -> Result<()> {
	let generated_at_text = generated_at.format(&Rfc3339)?;
	let generated_at_unix = generated_at.unix_timestamp();
	let transaction = connection.transaction()?;

	for candidate in candidates {
		transaction.execute(
			"INSERT OR REPLACE INTO protocol_event_summaries (
				run_id, event_count, last_sequence_number, last_event_type, last_event_at,
				last_event_at_unix, compacted_at, compacted_at_unix
			) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
			rusqlite::params![
				&candidate.run_id,
				i64::try_from(candidate.event_count).map_err(|_error| {
					eyre::eyre!(
						"Protocol event count for run `{}` overflowed i64.",
						candidate.run_id
					)
				})?,
				candidate.last_sequence_number,
				candidate.last_event_type.as_deref(),
				candidate.last_event_at.as_deref(),
				candidate.last_event_at_unix,
				&generated_at_text,
				generated_at_unix,
			],
		)?;
		transaction.execute(
			"DELETE FROM protocol_events WHERE run_id = ?1",
			rusqlite::params![&candidate.run_id],
		)?;
	}

	transaction.commit()?;

	Ok(())
}

fn copy_truncate(path: &Path, rotated_path: &Path) -> Result<()> {
	if let Some(parent) = rotated_path.parent() {
		fs::create_dir_all(parent)?;
	}

	fs::copy(path, rotated_path)?;
	OpenOptions::new().write(true).truncate(true).open(path)?;

	Ok(())
}

fn rotated_path(path: &Path, generated_at: OffsetDateTime) -> Result<PathBuf> {
	let parent = path.parent().ok_or_else(|| {
		eyre::eyre!("Maintenance target `{}` has no parent directory.", path.display())
	})?;
	let file_name = path.file_name().and_then(|name| name.to_str()).ok_or_else(|| {
		eyre::eyre!("Maintenance target `{}` has no UTF-8 file name.", path.display())
	})?;
	let Some((prefix, suffix)) = file_name.rsplit_once('.') else {
		return Ok(parent.join(format!("{file_name}.{}", generated_at.unix_timestamp())));
	};
	let candidate = parent.join(format!("{prefix}.{}.{suffix}", generated_at.unix_timestamp()));

	next_available_path(candidate)
}

fn next_available_path(path: PathBuf) -> Result<PathBuf> {
	if !path.exists() {
		return Ok(path);
	}

	let parent = path.parent().ok_or_else(|| {
		eyre::eyre!("Maintenance target `{}` has no parent directory.", path.display())
	})?;
	let file_name = path.file_name().and_then(|name| name.to_str()).ok_or_else(|| {
		eyre::eyre!("Maintenance target `{}` has no UTF-8 file name.", path.display())
	})?;

	for index in 1..=999 {
		let candidate = parent.join(format!("{file_name}.{index}"));

		if !candidate.exists() {
			return Ok(candidate);
		}
	}

	eyre::bail!("Could not allocate a unique maintenance rotation path for `{}`.", path.display());
}

fn file_is_older_than(metadata: &Metadata, system_now: SystemTime, retention: Duration) -> bool {
	metadata
		.modified()
		.ok()
		.and_then(|modified| system_now.duration_since(modified).ok())
		.is_some_and(|age| age > retention)
}

fn is_rotated_log_file(path: &Path) -> bool {
	path.file_stem()
		.and_then(|stem| stem.to_str())
		.and_then(|stem| stem.rsplit_once('.').map(|(_, timestamp)| timestamp))
		.is_some_and(|timestamp| timestamp.parse::<i64>().is_ok())
}

fn is_legacy_git_askpass_helper(path: &Path) -> bool {
	path.file_name().and_then(|name| name.to_str()).is_some_and(|file_name| {
		file_name.starts_with(LEGACY_GIT_ASKPASS_PREFIX)
			&& file_name.ends_with(LEGACY_GIT_ASKPASS_SUFFIX)
	})
}

fn collect_backup_candidates(
	root: &Path,
	groups: &mut BTreeMap<String, Vec<BackupCandidate>>,
) -> Result<()> {
	for entry in fs::read_dir(root)? {
		let entry = entry?;
		let path = entry.path();
		let file_type = entry.file_type()?;

		if file_type.is_dir() {
			collect_backup_candidates(&path, groups)?;

			continue;
		}
		if !file_type.is_file() {
			continue;
		}

		let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
			continue;
		};
		let Some((backup_prefix, _suffix)) = file_name.split_once(".bak-") else {
			continue;
		};
		let metadata = entry.metadata()?;
		let group_key = path
			.parent()
			.map(|parent| parent.join(backup_prefix).display().to_string())
			.unwrap_or_else(|| backup_prefix.to_owned());

		groups.entry(group_key).or_default().push(BackupCandidate {
			path,
			bytes: metadata.len(),
			modified: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
		});
	}

	Ok(())
}

fn print_prune_report(report: &MaintenanceReport) -> Result<()> {
	let mut stdout = io::stdout().lock();

	writeln!(stdout, "Decodex maintenance prune ({}, {})", report.mode, report.scope)?;
	writeln!(
		stdout,
		"logs: rotate {}/{} files ({} bytes), delete {}/{} files ({} bytes)",
		report.logs.rotated_files,
		report.logs.rotate_candidates,
		report.logs.rotate_bytes,
		report.logs.deleted_files,
		report.logs.delete_candidates,
		report.logs.delete_bytes
	)?;
	writeln!(
		stdout,
		"agent-evidence: rotate {}/{} streams ({} bytes), delete {}/{} files ({} bytes)",
		report.agent_evidence.rotated_files,
		report.agent_evidence.rotate_candidates,
		report.agent_evidence.rotate_bytes,
		report.agent_evidence.deleted_files,
		report.agent_evidence.delete_candidates,
		report.agent_evidence.delete_bytes
	)?;
	writeln!(
		stdout,
		"git-askpass: delete {}/{} files ({} bytes)",
		report.git_askpass_helpers.deleted_files,
		report.git_askpass_helpers.delete_candidates,
		report.git_askpass_helpers.delete_bytes
	)?;
	writeln!(
		stdout,
		"backups: delete {}/{} files ({} bytes)",
		report.backups.deleted_files, report.backups.delete_candidates, report.backups.delete_bytes
	)?;
	writeln!(
		stdout,
		"runtime: compact {}/{} terminal runs ({} protocol events), protected runs {}",
		report.runtime.compacted_runs,
		report.runtime.protocol_run_candidates,
		report.runtime.protocol_event_candidates,
		report.runtime.protected_run_count
	)?;

	for warning in &report.runtime.warnings {
		writeln!(stdout, "runtime warning: {} ({})", warning.warning, warning.reason)?;
	}

	match &report.wal_checkpoint {
		Some(checkpoint) => writeln!(
			stdout,
			"wal: {} checkpoint busy={} log_frames={} checkpointed_frames={}",
			checkpoint.mode, checkpoint.busy, checkpoint.log_frames, checkpoint.checkpointed_frames
		)?,
		None => writeln!(stdout, "wal: skipped")?,
	}

	Ok(())
}

#[cfg(test)]
mod tests {
	use std::{
		fs::{self, FileTimes, OpenOptions},
		path::Path,
		time::{Duration, SystemTime},
	};

	use rusqlite::OptionalExtension as _;
	use tempfile::TempDir;

	use crate::{
		maintenance::{
			self, Connection, MaintenanceMode, MaintenancePolicy, MaintenancePruneRequest,
			MaintenanceScope, OffsetDateTime,
		},
		test_support::TestEnvVarGuard,
	};

	const TEST_RUNTIME_SCHEMA: &str = "PRAGMA journal_mode = WAL;
		CREATE TABLE projects (
			service_id TEXT PRIMARY KEY NOT NULL,
			config_path TEXT NOT NULL,
			repo_root TEXT NOT NULL,
			worktree_root TEXT NOT NULL,
			workflow_path TEXT NOT NULL,
			tracker_api_key_env_var TEXT NOT NULL,
			github_token_env_var TEXT NOT NULL,
			enabled INTEGER NOT NULL,
			config_fingerprint TEXT NOT NULL,
			updated_at TEXT NOT NULL,
			updated_at_unix INTEGER NOT NULL
		);
		CREATE TABLE leases (
			issue_id TEXT PRIMARY KEY NOT NULL,
			project_id TEXT NOT NULL,
			run_id TEXT NOT NULL,
			issue_state TEXT NOT NULL
		);
		CREATE TABLE run_attempts (
			run_id TEXT PRIMARY KEY NOT NULL,
			project_id TEXT,
			issue_id TEXT NOT NULL,
			attempt_number INTEGER NOT NULL,
			status TEXT NOT NULL,
			thread_id TEXT,
			turn_id TEXT,
			updated_at TEXT NOT NULL,
			updated_at_unix INTEGER NOT NULL
		);
		CREATE TABLE protocol_events (
			run_id TEXT NOT NULL,
			sequence_number INTEGER NOT NULL,
			event_type TEXT NOT NULL,
			created_at TEXT NOT NULL,
			created_at_unix INTEGER NOT NULL,
			PRIMARY KEY (run_id, sequence_number)
		);
		CREATE TABLE worktrees (
			issue_id TEXT PRIMARY KEY NOT NULL,
			project_id TEXT NOT NULL,
			branch_name TEXT NOT NULL,
			worktree_path TEXT NOT NULL
		);
		CREATE TABLE linear_execution_events (
			idempotency_key TEXT PRIMARY KEY NOT NULL,
			service_id TEXT NOT NULL,
			issue_id TEXT NOT NULL,
			event_type TEXT NOT NULL,
			event_timestamp TEXT NOT NULL,
			event_unix INTEGER,
			payload_json TEXT NOT NULL,
			recorded_at TEXT NOT NULL,
			recorded_at_unix INTEGER NOT NULL
		);
		CREATE TABLE review_lifecycle_records (
			project_id TEXT NOT NULL,
			issue_id TEXT NOT NULL,
			branch_name TEXT NOT NULL,
			run_id TEXT NOT NULL,
			attempt_number INTEGER NOT NULL,
			pr_url TEXT NOT NULL,
			target_base_ref_name TEXT,
			pr_head_ref_name TEXT NOT NULL,
			pr_head_oid TEXT NOT NULL,
			head_sha TEXT NOT NULL,
			phase TEXT NOT NULL,
			request_comment_database_id INTEGER,
			request_created_at_unix_epoch INTEGER,
			request_description_thumbs_up_count INTEGER,
			request_retry_count INTEGER NOT NULL,
			external_round_count INTEGER NOT NULL,
			auto_merge_enabled_at_unix_epoch INTEGER,
			landing_state TEXT NOT NULL DEFAULT 'not_started',
			closeout_state TEXT NOT NULL DEFAULT 'not_started',
			repair_attempt_count INTEGER NOT NULL DEFAULT 0,
			evidence_json TEXT NOT NULL DEFAULT '{}',
			next_action TEXT NOT NULL DEFAULT '',
			updated_at TEXT NOT NULL,
			updated_at_unix INTEGER NOT NULL,
			PRIMARY KEY (project_id, issue_id, branch_name)
		);";

	#[test]
	fn prune_compacts_only_terminal_unowned_protocol_events() {
		let temp_dir = TempDir::new().expect("temp dir should create");
		let _home_guard =
			TestEnvVarGuard::set("HOME", temp_dir.path().to_str().expect("home should be utf-8"));
		let connection = bootstrap_test_runtime_db(&temp_dir);
		let now = OffsetDateTime::now_utc();
		let old = now.unix_timestamp() - 30 * 24 * 60 * 60;
		let fresh = now.unix_timestamp() - 2 * 24 * 60 * 60;

		insert_attempt(&connection, "old-run", "old-issue", "succeeded");
		insert_event(&connection, "old-run", 1, old);
		insert_event(&connection, "old-run", 2, old + 60);
		insert_attempt(&connection, "leased-run", "leased-issue", "running");
		insert_event(&connection, "leased-run", 1, old);
		insert_attempt(&connection, "old-leased-issue-run", "leased-issue", "succeeded");
		insert_event(&connection, "old-leased-issue-run", 1, old);

		connection
			.execute(
				"INSERT INTO leases (issue_id, project_id, run_id, issue_state)
				 VALUES ('leased-issue', 'decodex', 'leased-run', 'In Progress')",
				[],
			)
			.expect("run lease should insert");

		insert_attempt(&connection, "retained-run", "retained-issue", "failed");
		insert_event(&connection, "retained-run", 1, old);

		connection
			.execute(
				"INSERT INTO worktrees (issue_id, project_id, branch_name, worktree_path)
				 VALUES ('retained-issue', 'decodex', 'xy/retained', '/tmp/retained')",
				[],
			)
			.expect("retained worktree should insert");

		insert_attempt(&connection, "review-handoff-run", "review-issue", "succeeded");
		insert_event(&connection, "review-handoff-run", 1, old);
		insert_review_lifecycle(
			&connection,
			"review-issue",
			"review-handoff-run",
			"request_pending",
		);
		insert_attempt(&connection, "cleanup-blocked-run", "cleanup-issue", "succeeded");
		insert_event(&connection, "cleanup-blocked-run", 1, old);
		insert_review_lifecycle(
			&connection,
			"cleanup-issue",
			"cleanup-blocked-run",
			"cleanup_blocked",
		);
		insert_attempt(&connection, "attention-run", "attention-issue", "failed");
		insert_event(&connection, "attention-run", 1, old);
		insert_linear_execution_event(
			&connection,
			"attention-issue",
			"attention-run",
			"needs_attention",
		);
		insert_attempt(&connection, "terminal-failure-run", "failure-issue", "failed");
		insert_event(&connection, "terminal-failure-run", 1, old);
		insert_linear_execution_event(
			&connection,
			"failure-issue",
			"terminal-failure-run",
			"terminal_failure",
		);
		insert_attempt(&connection, "fresh-run", "fresh-issue", "succeeded");
		insert_event(&connection, "fresh-run", 1, fresh);

		let report = maintenance::run_prune_with_policy(
			MaintenancePruneRequest {
				mode: MaintenanceMode::Apply,
				scope: MaintenanceScope::Full,
				json: false,
			},
			MaintenancePolicy { protocol_event_retention_days: 14, ..MaintenancePolicy::default() },
		)
		.expect("maintenance should run");

		assert_eq!(report.runtime.compacted_runs, 1);
		assert_eq!(report.runtime.compacted_events, 2);
		assert_eq!(protocol_event_count(&connection, "old-run"), 0);
		assert_eq!(protocol_summary_event_count(&connection, "old-run"), Some(2));
		assert_eq!(protocol_event_count(&connection, "leased-run"), 1);
		assert_eq!(protocol_event_count(&connection, "old-leased-issue-run"), 1);
		assert_eq!(protocol_event_count(&connection, "retained-run"), 1);
		assert_eq!(protocol_event_count(&connection, "review-handoff-run"), 1);
		assert_eq!(protocol_event_count(&connection, "cleanup-blocked-run"), 1);
		assert_eq!(protocol_event_count(&connection, "attention-run"), 1);
		assert_eq!(protocol_event_count(&connection, "terminal-failure-run"), 1);
		assert_eq!(protocol_event_count(&connection, "fresh-run"), 1);
	}

	#[test]
	fn auto_safe_prune_compacts_terminal_unowned_protocol_events() {
		let temp_dir = TempDir::new().expect("temp dir should create");
		let _home_guard =
			TestEnvVarGuard::set("HOME", temp_dir.path().to_str().expect("home should be utf-8"));
		let connection = bootstrap_test_runtime_db(&temp_dir);
		let now = OffsetDateTime::now_utc();
		let old = now.unix_timestamp() - 30 * 24 * 60 * 60;

		insert_attempt(&connection, "old-run", "old-issue", "succeeded");
		insert_event(&connection, "old-run", 1, old);

		let report = maintenance::run_prune_with_policy(
			MaintenancePruneRequest {
				mode: MaintenanceMode::Apply,
				scope: MaintenanceScope::AutoSafe,
				json: false,
			},
			MaintenancePolicy { protocol_event_retention_days: 14, ..MaintenancePolicy::default() },
		)
		.expect("auto-safe maintenance should run");

		assert_eq!(report.runtime.compacted_runs, 1);
		assert_eq!(report.runtime.compacted_events, 1);
		assert!(report.runtime.warnings.is_empty());
		assert_eq!(protocol_event_count(&connection, "old-run"), 0);
		assert_eq!(protocol_summary_event_count(&connection, "old-run"), Some(1));

		let state_store =
			crate::state::StateStore::open(temp_dir.path().join(".codex/decodex/runtime.sqlite3"))
				.expect("state store should reopen compacted runtime DB");
		let runs = state_store
			.list_recent_runs("decodex", 10)
			.expect("recent runs should load compacted summary");
		let compacted_run = runs
			.iter()
			.find(|run| run.run_id() == "old-run")
			.expect("compacted run should remain status-visible");

		assert_eq!(compacted_run.event_count(), 1);
		assert_eq!(compacted_run.last_event_type(), Some("event"));
		assert_eq!(compacted_run.last_event_at(), Some("2026-05-01T00:00:00Z"));
	}

	#[test]
	fn auto_safe_prune_warns_and_continues_when_runtime_candidate_detection_fails() {
		let temp_dir = TempDir::new().expect("temp dir should create");
		let _home_guard =
			TestEnvVarGuard::set("HOME", temp_dir.path().to_str().expect("home should be utf-8"));
		let decodex_home = temp_dir.path().join(".codex/decodex");

		fs::create_dir_all(&decodex_home).expect("decodex home should create");
		Connection::open(decodex_home.join("runtime.sqlite3"))
			.expect("empty runtime DB should create");

		let report = maintenance::run_prune_with_policy(
			MaintenancePruneRequest {
				mode: MaintenanceMode::Apply,
				scope: MaintenanceScope::AutoSafe,
				json: false,
			},
			MaintenancePolicy::default(),
		)
		.expect("auto-safe maintenance should continue after candidate detection failure");

		assert_eq!(report.runtime.compacted_runs, 0);
		assert_eq!(report.runtime.warnings.len(), 1);
		assert_eq!(report.runtime.warnings[0].warning, "auto_protocol_event_compaction_skipped");
		assert_eq!(report.runtime.warnings[0].reason, "candidate_detection_failed");
	}

	#[test]
	fn prune_rotates_oversized_logs_and_agent_evidence_events() {
		let temp_dir = TempDir::new().expect("temp dir should create");
		let _home_guard =
			TestEnvVarGuard::set("HOME", temp_dir.path().to_str().expect("home should be utf-8"));
		let log_dir = temp_dir.path().join(".codex/decodex/logs");
		let evidence_dir = temp_dir.path().join(".codex/decodex/agent-evidence/decodex");
		let log_path = log_dir.join("decodex.log");
		let events_path = evidence_dir.join("events.jsonl");

		fs::create_dir_all(&log_dir).expect("log dir should create");
		fs::create_dir_all(&evidence_dir).expect("evidence dir should create");
		fs::write(&log_path, b"0123456789abcdef").expect("log should write");
		fs::write(&events_path, b"0123456789abcdef").expect("events should write");

		let report = maintenance::run_prune_with_policy(
			MaintenancePruneRequest {
				mode: MaintenanceMode::Apply,
				scope: MaintenanceScope::AutoSafe,
				json: false,
			},
			MaintenancePolicy {
				log_rotate_bytes: 8,
				evidence_rotate_bytes: 8,
				..MaintenancePolicy::default()
			},
		)
		.expect("maintenance should run");

		assert_eq!(report.logs.rotated_files, 1);
		assert_eq!(report.agent_evidence.rotated_files, 1);
		assert_eq!(fs::metadata(&log_path).expect("log should remain").len(), 0);
		assert_eq!(fs::metadata(&events_path).expect("events should remain").len(), 0);
		assert_eq!(
			fs::read_dir(&log_dir)
				.expect("log dir should list")
				.filter_map(std::result::Result::ok)
				.filter(|entry| entry.path() != log_path)
				.count(),
			1
		);
		assert_eq!(
			fs::read_dir(&evidence_dir)
				.expect("evidence dir should list")
				.filter_map(std::result::Result::ok)
				.filter(|entry| entry.path() != events_path)
				.count(),
			1
		);
	}

	#[test]
	fn prune_deletes_only_rotated_logs_and_agent_evidence_after_fourteen_days() {
		let temp_dir = TempDir::new().expect("temp dir should create");
		let _home_guard =
			TestEnvVarGuard::set("HOME", temp_dir.path().to_str().expect("home should be utf-8"));
		let log_dir = temp_dir.path().join(".codex/decodex/logs");
		let evidence_dir = temp_dir.path().join(".codex/decodex/agent-evidence/decodex");
		let current_log_path = log_dir.join("decodex.log");
		let old_log_path = log_dir.join("decodex.1.log");
		let fresh_log_path = log_dir.join("decodex.2.log");
		let current_events_path = evidence_dir.join("events.jsonl");
		let old_events_path = evidence_dir.join("events.1.jsonl");
		let fresh_events_path = evidence_dir.join("events.2.jsonl");
		let old_time = SystemTime::now() - Duration::from_secs(15 * 24 * 60 * 60);
		let fresh_time = SystemTime::now() - Duration::from_secs(2 * 24 * 60 * 60);

		fs::create_dir_all(&log_dir).expect("log dir should create");
		fs::create_dir_all(&evidence_dir).expect("evidence dir should create");

		for path in [
			&current_log_path,
			&old_log_path,
			&fresh_log_path,
			&current_events_path,
			&old_events_path,
			&fresh_events_path,
		] {
			fs::write(path, b"event\n").expect("maintenance fixture should write");
		}

		set_file_modified(&current_log_path, old_time);
		set_file_modified(&old_log_path, old_time);
		set_file_modified(&fresh_log_path, fresh_time);
		set_file_modified(&current_events_path, old_time);
		set_file_modified(&old_events_path, old_time);
		set_file_modified(&fresh_events_path, fresh_time);

		let report = maintenance::run_prune_with_policy(
			MaintenancePruneRequest {
				mode: MaintenanceMode::Apply,
				scope: MaintenanceScope::AutoSafe,
				json: false,
			},
			MaintenancePolicy::default(),
		)
		.expect("maintenance should run");

		assert_eq!(report.logs.deleted_files, 1);
		assert_eq!(report.agent_evidence.deleted_files, 1);
		assert!(current_log_path.exists());
		assert!(!old_log_path.exists());
		assert!(fresh_log_path.exists());
		assert!(current_events_path.exists());
		assert!(!old_events_path.exists());
		assert!(fresh_events_path.exists());
	}

	#[test]
	fn prune_deletes_old_legacy_git_askpass_helpers_from_registered_worktree_roots() {
		let temp_dir = TempDir::new().expect("temp dir should create");
		let _home_guard =
			TestEnvVarGuard::set("HOME", temp_dir.path().to_str().expect("home should be utf-8"));
		let connection = bootstrap_test_runtime_db(&temp_dir);
		let worktree_root = temp_dir.path().join("repo/.worktrees");
		let old_helper = worktree_root.join(".decodex-git-askpass-xy-101-attempt-1.sh");
		let fresh_helper = worktree_root.join(".decodex-git-askpass-xy-102-attempt-1.sh");
		let unrelated = worktree_root.join("notes.sh");
		let old_time = SystemTime::now() - Duration::from_secs(2 * 24 * 60 * 60);
		let fresh_time = SystemTime::now();

		insert_project(&connection, &worktree_root);

		fs::create_dir_all(&worktree_root).expect("worktree root should create");
		fs::write(&old_helper, b"#!/bin/sh\n").expect("old helper should write");
		fs::write(&fresh_helper, b"#!/bin/sh\n").expect("fresh helper should write");
		fs::write(&unrelated, b"#!/bin/sh\n").expect("unrelated file should write");

		set_file_modified(&old_helper, old_time);
		set_file_modified(&fresh_helper, fresh_time);
		set_file_modified(&unrelated, old_time);

		let report = maintenance::run_prune_with_policy(
			MaintenancePruneRequest {
				mode: MaintenanceMode::Apply,
				scope: MaintenanceScope::AutoSafe,
				json: false,
			},
			MaintenancePolicy::default(),
		)
		.expect("maintenance should run");

		assert_eq!(report.git_askpass_helpers.deleted_files, 1);
		assert_eq!(report.git_askpass_helpers.delete_candidates, 1);
		assert!(!old_helper.exists());
		assert!(fresh_helper.exists());
		assert!(unrelated.exists());
	}

	fn insert_attempt(connection: &Connection, run_id: &str, issue_id: &str, status: &str) {
		connection
			.execute(
				"INSERT INTO run_attempts (
					run_id, project_id, issue_id, attempt_number, status, updated_at, updated_at_unix
				) VALUES (?1, 'decodex', ?2, 1, ?3, '2026-05-01T00:00:00Z', 0)",
				rusqlite::params![run_id, issue_id, status],
			)
			.expect("attempt should insert");
	}

	fn insert_project(connection: &Connection, worktree_root: &Path) {
		connection
			.execute(
				"INSERT INTO projects (
					service_id, config_path, repo_root, worktree_root, workflow_path,
					tracker_api_key_env_var, github_token_env_var, enabled,
					config_fingerprint, updated_at, updated_at_unix
				) VALUES (
					'decodex', '/tmp/project.toml', '/tmp/repo', ?1, '/tmp/WORKFLOW.md',
					'LINEAR_API_KEY_HACKINK', 'GITHUB_PAT_Y', 1,
					'fingerprint', '2026-05-01T00:00:00Z', 0
				)",
				rusqlite::params![worktree_root.display().to_string()],
			)
			.expect("project should insert");
	}

	fn set_file_modified(path: &Path, modified: SystemTime) {
		OpenOptions::new()
			.write(true)
			.open(path)
			.expect("file should open for timestamp update")
			.set_times(FileTimes::new().set_modified(modified))
			.expect("file modified time should update");
	}

	fn bootstrap_test_runtime_db(temp_dir: &TempDir) -> Connection {
		let decodex_home = temp_dir.path().join(".codex/decodex");

		fs::create_dir_all(&decodex_home).expect("decodex home should create");

		let database_path = decodex_home.join("runtime.sqlite3");
		let connection = Connection::open(&database_path).expect("runtime DB should open");

		connection.execute_batch(TEST_RUNTIME_SCHEMA).expect("schema should bootstrap");

		maintenance::ensure_protocol_event_summary_table(&connection)
			.expect("summary table should create");

		connection
	}

	fn insert_event(connection: &Connection, run_id: &str, sequence_number: i64, created_at: i64) {
		connection
			.execute(
				"INSERT INTO protocol_events (
					run_id, sequence_number, event_type, created_at, created_at_unix
				) VALUES (?1, ?2, 'event', '2026-05-01T00:00:00Z', ?3)",
				rusqlite::params![run_id, sequence_number, created_at],
			)
			.expect("event should insert");
	}

	fn insert_review_lifecycle(connection: &Connection, issue_id: &str, run_id: &str, phase: &str) {
		connection
			.execute(
				"INSERT INTO review_lifecycle_records (
					project_id, issue_id, branch_name, run_id, attempt_number, pr_url,
					target_base_ref_name, pr_head_ref_name, pr_head_oid, head_sha, phase,
					request_comment_database_id,
					request_created_at_unix_epoch, request_description_thumbs_up_count,
					request_retry_count, external_round_count, auto_merge_enabled_at_unix_epoch,
					landing_state, closeout_state, repair_attempt_count, evidence_json,
					next_action,
					updated_at, updated_at_unix
				) VALUES (
					'decodex', ?1, 'y/decodex-test', ?2, 1,
					'https://github.com/hack-ink/decodex/pull/1', 'main',
					'y/decodex-test', 'abc123', 'abc123', ?3, NULL, NULL, NULL, 0, 0, NULL,
					'not_started', 'not_started', 0, '{}', '',
					'2026-05-01T00:00:00Z', 0
				)",
				rusqlite::params![issue_id, run_id, phase],
			)
			.expect("review lifecycle should insert");
	}

	fn insert_linear_execution_event(
		connection: &Connection,
		issue_id: &str,
		run_id: &str,
		event_type: &str,
	) {
		let idempotency_key = format!("{event_type}-{run_id}");
		let payload_json = serde_json::json!({
			"type": "decodex.linear_execution_event/1",
			"record_version": 1,
			"event_type": event_type,
			"event_timestamp": "2026-05-01T00:00:00Z",
			"idempotency_key": idempotency_key,
			"service_id": "decodex",
			"issue_id": issue_id,
			"issue_identifier": issue_id,
			"run_id": run_id,
			"attempt_number": 1
		})
		.to_string();

		connection
			.execute(
				"INSERT INTO linear_execution_events (
					idempotency_key, service_id, issue_id, event_type, event_timestamp,
					event_unix, payload_json, recorded_at, recorded_at_unix
				) VALUES (?1, 'decodex', ?2, ?3, '2026-05-01T00:00:00Z', 0, ?4,
					'2026-05-01T00:00:00Z', 0)",
				rusqlite::params![idempotency_key, issue_id, event_type, payload_json],
			)
			.expect("linear execution event should insert");
	}

	fn protocol_event_count(connection: &Connection, run_id: &str) -> i64 {
		connection
			.query_row(
				"SELECT COUNT(*) FROM protocol_events WHERE run_id = ?1",
				rusqlite::params![run_id],
				|row| row.get(0),
			)
			.expect("event count should read")
	}

	fn protocol_summary_event_count(connection: &Connection, run_id: &str) -> Option<i64> {
		connection
			.query_row(
				"SELECT event_count FROM protocol_event_summaries WHERE run_id = ?1",
				rusqlite::params![run_id],
				|row| row.get(0),
			)
			.optional()
			.expect("summary should read")
	}
}
