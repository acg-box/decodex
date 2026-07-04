use rusqlite::Connection;

use crate::{maintenance::reports::RuntimeProtocolCandidate, prelude::Result};

pub(in crate::maintenance::runtime) fn protocol_event_compaction_candidates(
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

pub(in crate::maintenance::runtime) fn protected_protocol_run_count(
	connection: &Connection,
) -> Result<usize> {
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
