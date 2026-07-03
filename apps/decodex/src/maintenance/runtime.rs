use std::{path::Path, time::Duration};

use color_eyre::Report;
use rusqlite::{self, Connection};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	maintenance::{
		policy::{MaintenanceMode, MaintenancePolicy, MaintenanceScope},
		reports::{
			RuntimeMaintenanceAction, RuntimeMaintenanceReport, RuntimeMaintenanceWarning,
			RuntimeProtocolCandidate, WalCheckpointReport,
		},
	},
	prelude::{Result, eyre},
	runtime,
};

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

pub(super) fn maintain_runtime(
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

pub(super) fn maintain_runtime_protocol_events(
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

pub(super) fn maintain_wal(
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
