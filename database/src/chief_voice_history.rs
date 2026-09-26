//! Read the existing voice transcript store with exact task and thread provenance.
use crate::{DatabaseError, SqliteStore, StoreError, error::sqlite_error};
use rusqlite::{Connection, params};

/// Append/closure version of one task thread's voice history.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ChiefVoiceHistoryRevision {
	pub last_event: i64,
	pub calls: i64,
	pub open_calls: i64,
	pub last_closed_at: i64,
}
/// One stored sentence, ordered by its call-local sequence.
#[derive(Clone, Debug)]
pub struct ChiefVoiceTranscript {
	pub sequence: i64,
	pub role: String,
	pub text: String,
	pub complete: Option<bool>,
}
/// A bounded excerpt from one authorized voice call.
#[derive(Clone, Debug)]
pub struct ChiefVoiceTranscriptCall {
	pub session_id: String,
	pub baseline_turn_id: Option<String>,
	pub entries: Vec<ChiefVoiceTranscript>,
}
/// Consistent bounded voice excerpts and their source version.
#[derive(Clone, Debug)]
pub struct ChiefVoiceHistory {
	pub revision: ChiefVoiceHistoryRevision,
	pub calls: Vec<ChiefVoiceTranscriptCall>,
	pub truncated: bool,
}

// Non-voice inbox source IDs are not JSON. Keep parsing inside CASE so query planning
// cannot evaluate json_extract on an unrelated record before the kind filter.
const VOICE_SESSION: &str = "CASE WHEN json_valid(e.source_event_id) THEN CASE WHEN json_extract(e.source_event_id,'$[0]')='voice_transcript' THEN json_extract(e.source_event_id,'$[1]') END END";
fn revision(
	connection: &Connection,
	work: &str,
	thread: &str,
) -> Result<ChiefVoiceHistoryRevision, StoreError> {
	let (calls, open_calls, last_closed_at) = connection.query_row("SELECT count(*),coalesce(sum(closed_at_micros IS NULL),0),coalesce(max(closed_at_micros),0) FROM chief_voice_calls WHERE work_id=?1 AND thread_id=?2",params![work,thread],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(sqlite_error)?;
	let last_event = connection.query_row(&format!("SELECT coalesce(max(e.id),0) FROM chief_inbox_events e JOIN chief_voice_calls c ON c.session_id={VOICE_SESSION} AND c.work_id=e.work_item_id WHERE c.work_id=?1 AND c.thread_id=?2 AND e.event_kind IN ('voice_user','voice_assistant')"),params![work,thread],|r|r.get(0)).map_err(sqlite_error)?;
	Ok(ChiefVoiceHistoryRevision { last_event, calls, open_calls, last_closed_at })
}
impl SqliteStore {
	/// Read the current version without loading transcript text or writing records.
	pub async fn chief_voice_history_revision(
		&self,
		work: String,
		thread: String,
	) -> Result<ChiefVoiceHistoryRevision, StoreError> {
		self.run(move |connection| revision(connection, &work, &thread)).await
	}

	/// Read up to eight recent calls and 32 recent sentences per call in a single snapshot.
	pub async fn read_chief_voice_history(
		&self,
		work: String,
		thread: String,
	) -> Result<ChiefVoiceHistory, StoreError> {
		self.run(move |connection| {
   let tx = connection.transaction().map_err(sqlite_error)?;
   let revision = revision(&tx, &work, &thread)?;
   let selected = {
    let mut query = tx.prepare("SELECT session_id,baseline_turn_id FROM chief_voice_calls WHERE work_id=?1 AND thread_id=?2 ORDER BY rowid DESC LIMIT 8").map_err(sqlite_error)?;
    query.query_map(params![work,thread],|r|Ok((r.get::<_,String>(0)?,r.get::<_,Option<String>>(1)?))).map_err(sqlite_error)?.collect::<Result<Vec<_>,_>>().map_err(sqlite_error)?
   };
   let mut truncated = revision.calls > 8;
   let mut calls = Vec::new();
   for (session_id, baseline_turn_id) in selected.into_iter().rev() {
    let sql = format!("SELECT json_extract(e.source_event_id,'$[2]'),e.event_kind,json_extract(e.payload,'$.text'),json_extract(e.payload,'$.complete') FROM chief_inbox_events e WHERE e.work_item_id=?1 AND e.event_kind IN ('voice_user','voice_assistant') AND {VOICE_SESSION}=?2 ORDER BY json_extract(e.source_event_id,'$[2]') DESC LIMIT 33");
    let mut query = tx.prepare(&sql).map_err(sqlite_error)?;
    let mut entries = query.query_map(params![work,session_id],|r|Ok(ChiefVoiceTranscript{sequence:r.get(0)?,role:r.get::<_,String>(1)?.trim_start_matches("voice_").into(),text:r.get(2)?,complete:r.get(3)?})).map_err(sqlite_error)?.collect::<Result<Vec<_>,_>>().map_err(sqlite_error)?;
    truncated |= entries.len() > 32;
    entries.truncate(32);
    entries.reverse();
    if entries.iter().any(|e| e.text.len() > 32768 || e.sequence <= 0) || entries.windows(2).any(|p| p[0].sequence >= p[1].sequence) {
     return Err(DatabaseError::Conflict.into());
    }
    calls.push(ChiefVoiceTranscriptCall { session_id, baseline_turn_id, entries });
   }
   tx.commit().map_err(sqlite_error)?;
   Ok(ChiefVoiceHistory { revision, calls, truncated })
  }).await
	}
}
