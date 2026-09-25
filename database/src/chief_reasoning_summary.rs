//! Public summary parts share the bounded live-output owner, never the raw reasoning payload.
use crate::{SqliteStore, StoreError, error::sqlite_error};
use rusqlite::{OptionalExtension as _, params};

pub enum ChiefReasoningSummaryChange {
	VoiceHandoff,
	Delta { index: usize, text: String },
	Completed { parts: Vec<String> },
}

impl SqliteStore {
	pub async fn update_chief_reasoning_summary(
		&self,
		thread: String,
		turn: String,
		item: String,
		generation: Option<String>,
		change: ChiefReasoningSummaryChange,
	) -> Result<(), StoreError> {
		if [&thread, &turn, &item].iter().any(|id| id.is_empty() || id.len() > 512) {
			return Err(StoreError::InvalidInput("invalid reasoning summary identity"));
		}
		let changed = self.run(move |connection| {
			let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(sqlite_error)?;
			let work: Option<String> = tx.query_row("SELECT id FROM chief_work_items WHERE codex_thread_id=?1 AND active_turn_id=?2 AND dispatch_state='running'", params![thread,turn], |row|row.get(0)).optional().map_err(sqlite_error)?;
			let Some(work) = work else { return Ok(false); };
			if !crate::chief_process::owns_work(&tx, &work, generation.as_deref())? { return Ok(false); }
			let voice_source = serde_json::json!(["reasoning_voice_handoff",work,turn]).to_string();
			if matches!(change, ChiefReasoningSummaryChange::VoiceHandoff) {
				let now = crate::unix_micros()?;
				let typed = tx.prepare("SELECT item_id FROM chief_live_output WHERE work_id=?1 AND turn_id=?2 AND kind='reasoningSummary'").map_err(sqlite_error)?
					.query_map(params![work,turn], |row|row.get::<_, String>(0)).map_err(sqlite_error)?.collect::<Result<Vec<_>,_>>().map_err(sqlite_error)?;
				let payload = serde_json::json!({"typedItemIds":typed}).to_string();
				tx.execute("INSERT INTO chief_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros,delivery_work_item_id,delivered_turn_id) VALUES(?1,?2,'reasoning_voice_handoff',?5,?3,'resolved','Native voice provenance',?3,?2,?4) ON CONFLICT(source_event_id) DO NOTHING",params![voice_source,work,now,turn,payload]).map_err(sqlite_error)?;
				tx.commit().map_err(sqlite_error)?;
				return Ok(true);
			}
			let prior: Option<(String, bool, bool, Option<String>)> = tx.query_row("SELECT kind,completed,truncated,summary_parts FROM chief_live_output WHERE work_id=?1 AND turn_id=?2 AND item_id=?3", params![work,turn,item], |row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).optional().map_err(sqlite_error)?;
			if prior.as_ref().is_some_and(|(kind, _, _, _)| kind != "reasoningSummary") {
				return Err(StoreError::InvalidInput("live output kind changed"));
			}
			if prior.is_none() {
				let delegated: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM chief_inbox_events WHERE source_event_id=?1)", [voice_source], |row|row.get(0)).map_err(sqlite_error)?;
				if delegated { return Ok(false); }
				let count: i64 = tx.query_row("SELECT count(*) FROM chief_live_output WHERE work_id=?1 AND turn_id=?2", params![work,turn], |row|row.get(0)).map_err(sqlite_error)?;
				if count >= 32 { return Ok(false); }
			}
			let (mut parts, completed, mut truncated) = match change {
				ChiefReasoningSummaryChange::VoiceHandoff => unreachable!("handled before output"),
				ChiefReasoningSummaryChange::Completed { parts } => (parts, true, false),
				ChiefReasoningSummaryChange::Delta { index, text } => {
					if prior.as_ref().is_some_and(|(_, completed, _, _)| *completed) { return Ok(false); }
					if index >= 32 {
						tx.execute("INSERT INTO chief_live_output(work_id,turn_id,item_id,kind,truncated,summary_parts) VALUES(?1,?2,?3,'reasoningSummary',1,'[]') ON CONFLICT(work_id,turn_id,item_id) DO UPDATE SET truncated=1",params![work,turn,item]).map_err(sqlite_error)?;
						tx.commit().map_err(sqlite_error)?;
						return Ok(true);
					}
					let mut parts: Vec<String> = prior.as_ref().and_then(|(_, _, _, parts)| parts.as_deref())
						.map(serde_json::from_str).transpose().map_err(|_| StoreError::InvalidInput("invalid stored summary parts"))?.unwrap_or_default();
					parts.resize_with(parts.len().max(index + 1), String::new);
					parts[index].push_str(&text);
					(parts, false, prior.is_some_and(|(_, _, truncated, _)| truncated))
				},
			};
			truncated |= parts.len() > 32;
			parts.truncate(32);
			let mut remaining = 65536usize.saturating_sub(parts.len().saturating_sub(1) * 2);
			for part in &mut parts {
				let end = part.floor_char_boundary(remaining.min(part.len()));
				truncated |= end < part.len();
				part.truncate(end);
				remaining -= end;
			}
			let text = parts.join("\n\n");
			let parts = serde_json::to_string(&parts).map_err(|_| StoreError::InvalidInput("invalid summary parts"))?;
			tx.execute("DELETE FROM chief_live_output WHERE work_id=?1 AND turn_id<>?2", params![work,turn]).map_err(sqlite_error)?;
			tx.execute("INSERT INTO chief_live_output(work_id,turn_id,item_id,text,truncated,kind,completed,summary_parts) VALUES(?1,?2,?3,?4,?5,'reasoningSummary',?6,?7) ON CONFLICT(work_id,turn_id,item_id) DO UPDATE SET text=excluded.text,truncated=excluded.truncated,completed=excluded.completed,summary_parts=excluded.summary_parts", params![work,turn,item,text,truncated,completed,parts]).map_err(sqlite_error)?;
			tx.commit().map_err(sqlite_error)?;
			Ok(true)
		}).await?;
		if changed {
			self.inner
				.chief_output_revision
				.send_modify(|revision| *revision = revision.wrapping_add(1));
		}
		Ok(())
	}

	/// A recorded handoff hides later summaries. Retain only items seen before it.
	pub async fn read_chief_reasoning_origins(
		&self,
		work: String,
		thread: String,
		turns: Vec<String>,
	) -> Result<Vec<(String, Vec<String>)>, StoreError> {
		if turns.len() > 100 {
			return Err(StoreError::InvalidInput("too many reasoning turns"));
		}
		self.run(move |connection| {
			let mut statement = connection.prepare("SELECT e.payload FROM chief_inbox_events e JOIN chief_work_items w ON w.id=e.work_item_id WHERE e.source_event_id=?1 AND w.id=?2 AND w.codex_thread_id=?3").map_err(sqlite_error)?;
			let mut result = Vec::new();
			for turn in turns {
				let source = serde_json::json!(["reasoning_voice_handoff",work,turn]).to_string();
				let payload: Option<String> = statement.query_row(params![source,work,thread], |row|row.get(0)).optional().map_err(sqlite_error)?;
				if let Some(payload) = payload {
					let value: serde_json::Value = serde_json::from_str(&payload).map_err(|_| StoreError::InvalidInput("invalid reasoning provenance"))?;
					let typed = value["typedItemIds"].as_array().into_iter().flatten().filter_map(|id|id.as_str().map(str::to_owned)).collect();
					result.push((turn,typed));
				}
			}
			Ok(result)
		}).await
	}
}
