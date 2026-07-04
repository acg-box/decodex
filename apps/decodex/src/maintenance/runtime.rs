mod compaction;
mod protocol;
mod wal;
mod warnings;

use std::{path::Path, time::Duration};

use rusqlite::Connection;
use time::OffsetDateTime;

use crate::{
	maintenance::{
		policy::{MaintenanceMode, MaintenancePolicy, MaintenanceScope},
		reports::{RuntimeMaintenanceAction, RuntimeMaintenanceReport, WalCheckpointReport},
	},
	prelude::Result,
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
			let warning = warnings::runtime_maintenance_warning_for_error(&error);

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
	let candidates = protocol::protocol_event_compaction_candidates(&connection, cutoff_unix)?;

	report.protocol_run_candidates = candidates.len();
	report.protocol_event_candidates =
		candidates.iter().map(|candidate| candidate.event_count).sum::<u64>();
	report.protected_run_count = protocol::protected_protocol_run_count(&connection)?;

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
		compaction::compact_protocol_events(&mut connection, &candidates, generated_at)?;

		report.compacted_runs = candidates.len();
		report.compacted_events = report.protocol_event_candidates;
	}

	Ok(())
}

pub(super) fn maintain_wal(
	mode: MaintenanceMode,
	scope: MaintenanceScope,
) -> Result<Option<WalCheckpointReport>> {
	wal::maintain_wal(mode, scope)
}
