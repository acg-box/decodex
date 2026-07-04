use rusqlite::Connection;

use crate::{
	maintenance::{
		policy::{MaintenanceMode, MaintenanceScope},
		reports::WalCheckpointReport,
	},
	prelude::Result,
	runtime,
};

pub(in crate::maintenance::runtime) fn maintain_wal(
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
