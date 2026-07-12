use std::cmp::Ordering;
#[cfg(test)] use std::path::PathBuf;

use rusqlite::{self, Error, Row};

use crate::state::{
	ChildAgentActivitySummary, RunActivitySummaryRecord, RunAttemptRecord,
	runtime_row_parsers::common,
};
#[cfg(test)] use crate::state::WorktreeMappingRecord;

pub(in crate::state) fn compare_attempt_records(
	left: &RunAttemptRecord,
	right: &RunAttemptRecord,
) -> Ordering {
	left.attempt_number
		.cmp(&right.attempt_number)
		.then_with(|| left.updated_at_unix.cmp(&right.updated_at_unix))
		.then_with(|| left.run_id.cmp(&right.run_id))
}

pub(in crate::state) fn run_attempt_record_from_row(
	row: &Row<'_>,
) -> std::result::Result<RunAttemptRecord, Error> {
	Ok(RunAttemptRecord {
		run_id: row.get(0)?,
		project_id: row.get(1)?,
		issue_id: row.get(2)?,
		attempt_number: row.get(3)?,
		status: row.get(4)?,
		thread_id: row.get(5)?,
		turn_id: row.get(6)?,
		updated_at: row.get(7)?,
		updated_at_unix: row.get(8)?,
	})
}

pub(in crate::state) fn run_activity_summary_record_from_row(
	row: &Row<'_>,
) -> std::result::Result<RunActivitySummaryRecord, Error> {
	Ok(RunActivitySummaryRecord {
		run_id: row.get(0)?,
		attempt_number: row.get(1)?,
		child_agent_activity: common::optional_json_from_row::<ChildAgentActivitySummary>(row, 2)?
			.map(ChildAgentActivitySummary::sealed_durable),
		protocol_activity: common::optional_json_from_row(row, 3)?,
		updated_at: row.get(4)?,
		updated_at_unix: row.get(5)?,
	})
}

#[cfg(test)]
pub(in crate::state) fn worktree_mapping_record_from_row(
	row: &Row<'_>,
) -> std::result::Result<WorktreeMappingRecord, Error> {
	Ok(WorktreeMappingRecord {
		issue_id: row.get(0)?,
		project_id: row.get(1)?,
		branch_name: row.get(2)?,
		worktree_path: PathBuf::from(row.get::<_, String>(3)?),
		provenance_source: row.get(4)?,
		created_at_unix: row.get(5)?,
		updated_at_unix: row.get(6)?,
	})
}
