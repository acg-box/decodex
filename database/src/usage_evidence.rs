//! Aggregate exact retained turn receipts and explicit Work references. No execution writes.
use rusqlite::{OptionalExtension as _, params};
use serde_json::Value;

use crate::{SqliteStore, StoreError, error::sqlite_error};

#[derive(Default)]
pub struct UsageEvidence {
	pub turns: u64,
	pub token_turns: u64,
	pub duration_turns: u64,
	pub shared_turns: u64,
	pub unattributed_turns: u64,
	pub input_tokens: u64,
	pub output_tokens: u64,
	pub duration_ms: u64,
	pub sources: Vec<UsageEvidenceSource>,
}

pub struct UsageEvidenceSource {
	pub event_id: Option<i64>,
	pub agent_id: String,
	pub thread_id: String,
	pub turn_id: String,
	pub work_ids: Vec<String>,
	pub work_count: u64,
	pub input_tokens: Option<u64>,
	pub output_tokens: Option<u64>,
	pub duration_ms: Option<u64>,
}

// Results, native delivery proofs and acknowledged peer instructions identify exact turns.
// Current Agent selection, latest output, titles and cumulative token samples do not.
const SOURCES: &str = "
WITH links AS (
 SELECT r.agent_id,json_extract(r.record_json,'$.source.thread_id') thread_id,
 json_extract(r.record_json,'$.source.turn_id') turn_id,r.work_id FROM work_results r
 WHERE json_extract(r.record_json,'$.source.kind')='native_item'
 UNION SELECT w.owner_agent_id,d.child_thread_id,d.child_turn_id,n.work_id
 FROM native_work_deliveries d JOIN native_work_contributions n ON n.id=d.contribution_id
 JOIN works w ON w.id=n.work_id
 UNION SELECT q.recipient_agent_id,json_extract(d.payload,'$.threadId'),e.delivered_turn_id,q.work_id
 FROM collaboration_instructions i JOIN collaboration_requests q ON q.id=i.request_id
 JOIN agent_inbox_events e ON e.id=i.event_id
 JOIN agent_inbox_events d ON d.source_event_id=json_array('collaboration_delivery',e.id) AND d.event_kind='collaboration_delivery' AND d.work_item_id=q.recipient_agent_id
 WHERE json_type(d.payload,'$.threadId')='text' AND json_extract(d.payload,'$.turnId')=e.delivered_turn_id AND e.delivered_turn_id<>''
), receipts AS (
 SELECT e.id,e.work_item_id agent_id,json_extract(e.payload,'$.terminal.threadId') thread_id,
 json_extract(e.payload,'$.terminal.turn.id') turn_id,json_extract(e.payload,'$.usage') usage,
 json_extract(e.payload,'$.terminal.turn.durationMs') duration
 FROM agent_inbox_events e WHERE e.event_kind IN('agent_turn_completed','worker_turn_completed','capacity_retry')
 AND json_valid(e.payload) AND json_type(e.payload,'$.terminal.threadId')='text'
 AND json_type(e.payload,'$.terminal.turn.id')='text'
 AND json_extract(e.payload,'$.terminal.turn.status') IN('completed','failed','interrupted')
 AND e.source_event_id=json_array('turn/completed',json_extract(e.payload,'$.terminal.threadId'),json_extract(e.payload,'$.terminal.turn.id'))
), identities AS (
 SELECT agent_id,thread_id,turn_id FROM receipts UNION SELECT agent_id,thread_id,turn_id FROM links
)
SELECT r.id,k.agent_id,k.thread_id,k.turn_id,r.usage,r.duration,
 (SELECT count(*) FROM links l WHERE l.agent_id=k.agent_id AND l.thread_id=k.thread_id AND l.turn_id=k.turn_id),
 (SELECT json_group_array(work_id) FROM (SELECT work_id FROM links l WHERE l.agent_id=k.agent_id AND l.thread_id=k.thread_id AND l.turn_id=k.turn_id ORDER BY work_id LIMIT 4))
FROM identities k LEFT JOIN receipts r ON r.agent_id=k.agent_id AND r.thread_id=k.thread_id AND r.turn_id=k.turn_id
WHERE (?1 AND k.agent_id=?2) OR (NOT ?1 AND EXISTS(SELECT 1 FROM links l WHERE l.agent_id=k.agent_id AND l.thread_id=k.thread_id AND l.turn_id=k.turn_id AND l.work_id=?2))
ORDER BY r.id DESC,k.agent_id,k.thread_id,k.turn_id";

fn add(sum: &mut u64, value: u64) -> Result<(), StoreError> {
	*sum = sum
		.checked_add(value)
		.ok_or(StoreError::InvalidInput("usage total exceeds supported range"))?;
	Ok(())
}

impl SqliteStore {
	/// Read known totals across retained bindings, with at most 32 exact source examples.
	/// Shared turns are visible but excluded from Work totals. Agent totals count them once.
	pub async fn read_usage_evidence(
		&self,
		agent_scope: bool,
		id: String,
	) -> Result<Option<UsageEvidence>, StoreError> {
		self.run(move |c| {
			let tx = c.transaction().map_err(sqlite_error)?;
			let table = if agent_scope { "agents" } else { "works" };
			if tx
				.query_row(&format!("SELECT id FROM {table} WHERE id=?1"), [&id], |r| {
					r.get::<_, String>(0)
				})
				.optional()
				.map_err(sqlite_error)?
				.is_none()
			{
				return Ok(None);
			}
			let mut total = UsageEvidence::default();
			{
				let mut statement = tx.prepare(SOURCES).map_err(sqlite_error)?;
				let mut rows = statement.query(params![agent_scope, id]).map_err(sqlite_error)?;
				while let Some(row) = rows.next().map_err(sqlite_error)? {
					let usage: Option<String> = row.get(4).map_err(sqlite_error)?;
					let value = usage.and_then(|s| serde_json::from_str::<Value>(&s).ok());
					let tokens = value.as_ref().and_then(|v| {
						Some((v["input_tokens"].as_u64()?, v["output_tokens"].as_u64()?))
					});
					let duration: Option<i64> = row.get(5).map_err(sqlite_error)?;
					let duration = duration.and_then(|v| u64::try_from(v).ok());
					let work_count = u64::try_from(row.get::<_, i64>(6).map_err(sqlite_error)?)
						.map_err(|_| StoreError::InvalidInput("invalid attribution count"))?;
					total.turns += 1;
					total.shared_turns += u64::from(work_count > 1);
					total.unattributed_turns += u64::from(work_count == 0);
					if agent_scope || work_count == 1 {
						if let Some((input, output)) = tokens {
							add(&mut total.input_tokens, input)?;
							add(&mut total.output_tokens, output)?;
							total.token_turns += 1;
						}
						if let Some(ms) = duration {
							add(&mut total.duration_ms, ms)?;
							total.duration_turns += 1;
						}
					}
					if total.sources.len() < 32 {
						let work_json: String = row.get(7).map_err(sqlite_error)?;
						total.sources.push(UsageEvidenceSource {
							event_id: row.get(0).map_err(sqlite_error)?,
							agent_id: row.get(1).map_err(sqlite_error)?,
							thread_id: row.get(2).map_err(sqlite_error)?,
							turn_id: row.get(3).map_err(sqlite_error)?,
							work_ids: serde_json::from_str(&work_json).map_err(|_| {
								StoreError::InvalidInput("invalid Work attribution")
							})?,
							work_count,
							input_tokens: tokens.map(|t| t.0),
							output_tokens: tokens.map(|t| t.1),
							duration_ms: duration,
						});
					}
				}
			}
			tx.commit().map_err(sqlite_error)?;
			Ok(Some(total))
		})
		.await
	}
}

#[cfg(test)]
#[path = "usage_evidence_tests.rs"]
mod tests;
