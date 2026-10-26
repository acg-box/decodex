//! A parent receives results only while its explicit assignment is outstanding.
use rusqlite::Connection;

use crate::{StoreError, error};

pub(super) fn begin(c: &Connection, work: &str, instruction: i64) -> Result<(), StoreError> {
	c.execute(
		"INSERT INTO agent_delegations(work_id,instruction_event_id)
		 SELECT id,?2 FROM agent_work_items WHERE id=?1 AND parent_goal_id IS NOT NULL
		 ON CONFLICT(work_id) DO UPDATE SET instruction_event_id=excluded.instruction_event_id",
		rusqlite::params![work, instruction],
	)
	.map_err(crate::error::sqlite_error)?;

	Ok(())
}

pub(super) fn awaiting_result(c: &Connection, work: &str) -> Result<bool, StoreError> {
	c.query_row("SELECT EXISTS(SELECT 1 FROM agent_delegations WHERE work_id=?1)", [work], |row| {
		row.get(0)
	})
	.map_err(|error| error::sqlite_error(error).into())
}

pub(super) fn accept(c: &Connection, work: &str, result: i64) -> Result<(), StoreError> {
	// An older result cannot retire a newer assignment dispatched during its review.
	c.execute(
		"DELETE FROM agent_delegations WHERE work_id=?1 AND instruction_event_id<?2",
		rusqlite::params![work, result],
	)
	.map_err(crate::error::sqlite_error)?;

	Ok(())
}
