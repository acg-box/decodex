use rusqlite::{self, Error, Row};

use crate::state::ConnectorBackoff;

pub(in crate::state) fn connector_backoff_from_row(
	row: &Row<'_>,
) -> std::result::Result<ConnectorBackoff, Error> {
	Ok(ConnectorBackoff {
		project_id: row.get(0)?,
		connector: row.get(1)?,
		sync_phase: row.get(2)?,
		quota_class: row.get(3)?,
		reset_unix_epoch: row.get(4)?,
		reset_source: row.get(5)?,
		warning: row.get(6)?,
		updated_at: row.get(7)?,
		updated_at_unix: row.get(8)?,
	})
}
