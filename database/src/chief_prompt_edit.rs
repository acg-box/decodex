//! Durable history-edit intent. A reservation is never permission to retry a native write.
use crate::{SqliteStore, StoreError, chief_process::owns_work, error::sqlite_error, unix_micros};
use rusqlite::{Connection, OptionalExtension as _, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ChiefPromptEditAttempt {
	pub work: String,
	pub thread: String,
	pub generation: Option<String>,
	pub review_token: String,
	pub attempt_id: String,
	pub before_turn_id: String,
	pub item_id: String,
	/// Complete chronological native turn IDs from the reviewed history.
	pub turn_ids: Vec<String>,
	/// Complete canonical native input. Never replace this with rendered UI text.
	pub content: Vec<Value>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChiefPromptEditReceipt {
	pub id: i64,
	pub attempt: ChiefPromptEditAttempt,
	pub state: String,
}

impl ChiefPromptEditAttempt {
	fn validate(&self) -> Result<(), StoreError> {
		let valid =
			|v: &str| !v.trim().is_empty() && v.len() <= 512 && !v.chars().any(char::is_control);
		let mut unique = std::collections::BTreeSet::new();
		if [&self.work, &self.thread, &self.attempt_id, &self.before_turn_id, &self.item_id]
			.into_iter()
			.chain(self.generation.iter())
			.any(|v| !valid(v))
			|| self.review_token.len() != 64
			|| !self.review_token.bytes().all(|v| v.is_ascii_hexdigit())
			|| self.turn_ids.is_empty()
			|| self.turn_ids.iter().any(|id| !valid(id) || !unique.insert(id))
			|| !self.turn_ids.contains(&self.before_turn_id)
			|| self.content.is_empty()
			|| self.content.iter().any(|v| !v.is_object())
			|| serde_json::to_vec(self)
				.map_err(|_| StoreError::InvalidInput("invalid prompt edit"))?
				.len() > 8 * 1024 * 1024
		{
			return Err(StoreError::InvalidInput("invalid prompt edit"));
		}
		Ok(())
	}

	fn key(&self) -> String {
		let digest = Sha256::digest(
			json!([self.work, self.thread, self.review_token]).to_string().as_bytes(),
		);
		format!(
			"prompt-edit:{}",
			digest.iter().map(|byte| format!("{byte:02x}")).collect::<String>()
		)
	}
}

pub(crate) fn pending(c: &Connection, work: &str) -> Result<bool, StoreError> {
	c.query_row("SELECT EXISTS(SELECT 1 FROM chief_inbox_events a WHERE a.work_item_id=?1 AND a.event_kind='prompt_edit_attempt' AND NOT EXISTS(SELECT 1 FROM chief_inbox_events r WHERE r.source_event_id=a.source_event_id||':release' AND r.event_kind='prompt_edit_release'))",[work],|r|r.get(0)).map_err(|e|sqlite_error(e).into())
}

fn receipt(c: &Connection, id: i64) -> Result<Option<ChiefPromptEditReceipt>, StoreError> {
	let row: Option<(String,String)> = c.query_row("SELECT a.payload,coalesce(r.disposition_note,o.disposition_note,'reserved') FROM chief_inbox_events a LEFT JOIN chief_inbox_events o ON o.source_event_id=a.source_event_id||':observation' AND o.event_kind='prompt_edit_observation' LEFT JOIN chief_inbox_events r ON r.source_event_id=a.source_event_id||':release' AND r.event_kind='prompt_edit_release' WHERE a.id=?1 AND a.event_kind='prompt_edit_attempt'",[id],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(sqlite_error)?;
	row.map(|(payload, state)| {
		Ok(ChiefPromptEditReceipt {
			id,
			attempt: serde_json::from_str(&payload)
				.map_err(|_| StoreError::InvalidInput("invalid saved prompt edit"))?,
			state,
		})
	})
	.transpose()
}
fn owned(
	c: &Connection,
	a: &ChiefPromptEditAttempt,
	generation: Option<&str>,
) -> Result<bool, StoreError> {
	Ok(owns_work(c, &a.work, generation)?
		&& c.query_row(
			"SELECT EXISTS(SELECT 1 FROM chief_work_items WHERE id=?1 AND codex_thread_id=?2)",
			params![a.work, a.thread],
			|r| r.get::<_, bool>(0),
		)
		.map_err(sqlite_error)?)
}
fn release(c: &Connection, a: &ChiefPromptEditAttempt, state: &str) -> Result<bool, StoreError> {
	let now = unix_micros()?;
	Ok(c.execute("INSERT OR IGNORE INTO chief_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES(?1,?2,'prompt_edit_release','{}',?3,'resolved',?4,?3)",params![format!("{}:release",a.key()),a.work,now,state]).map_err(sqlite_error)? == 1)
}

impl SqliteStore {
	/// Reserve only reviewed, idle, unqueued input. Bind native evidence to a live guard in
	/// runtime. Repeated review tokens never create another attempt, including after a release.
	pub async fn reserve_chief_prompt_edit(
		&self,
		a: ChiefPromptEditAttempt,
	) -> Result<Option<i64>, StoreError> {
		a.validate()?;
		self.run(move |c| {
			let tx = c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sqlite_error)?;
			if !owned(&tx,&a,a.generation.as_deref())? || pending(&tx,&a.work)?
				|| crate::chief_models::pending(&tx,&a.work)? || crate::chief_permissions::pending(&tx,&a.work)? || crate::chief_plugins::pending(&tx,&a.work)? { return Ok(None); }
			let eligible: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM chief_work_items w WHERE w.id=?1 AND w.dispatch_state='idle' AND w.active_turn_id IS NULL AND w.status<>'resolved' AND NOT EXISTS(SELECT 1 FROM chief_voice_calls v WHERE v.work_id=w.id AND v.closed_at_micros IS NULL) AND NOT EXISTS(SELECT 1 FROM chief_inbox_events e WHERE (e.work_item_id=w.id OR e.delivery_work_item_id=w.id) AND e.disposition IS NULL AND (e.delivered_turn_id IS NULL OR e.delivered_turn_id='')))",[&a.work],|r|r.get(0)).map_err(sqlite_error)?;
			if !eligible { return Ok(None); }
			let now=unix_micros()?;
			let changed=tx.execute("INSERT OR IGNORE INTO chief_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES(?1,?2,'prompt_edit_attempt',?3,?4,'resolved','reserved',?4)",params![a.key(),a.work,json!(a).to_string(),now]).map_err(sqlite_error)?;
			let id=tx.last_insert_rowid(); tx.commit().map_err(sqlite_error)?;
			Ok((changed==1).then_some(id))
		}).await
	}

	/// Read durable evidence after reconnect; absence of a reply never grants replay.
	pub async fn chief_prompt_edit_receipt(
		&self,
		work: String,
		thread: String,
	) -> Result<Option<ChiefPromptEditReceipt>, StoreError> {
		self.run(move |c| {
			let id: Option<i64> = c.query_row("SELECT id FROM chief_inbox_events WHERE work_item_id=?1 AND event_kind='prompt_edit_attempt' AND json_extract(payload,'$.thread')=?2 ORDER BY id DESC LIMIT 1",params![work,thread],|r|r.get(0)).optional().map_err(sqlite_error)?;
			id.map(|id| receipt(c,id)).transpose().map(Option::flatten)
		}).await
	}

	/// Use only positive pre-write or native validation rejection evidence; never a timeout
	/// or generic remote error. Native -32602..=-32600 errors leave history unchanged.
	pub async fn reject_chief_prompt_edit_without_mutation(
		&self,
		id: i64,
		a: ChiefPromptEditAttempt,
	) -> Result<bool, StoreError> {
		a.validate()?;
		self.run(move |c| {
			let tx = c
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(sqlite_error)?;
			if !owned(&tx, &a, a.generation.as_deref())?
				|| !receipt(&tx, id)?.is_some_and(|r| r.attempt == a && r.state == "reserved")
			{
				return Ok(false);
			}
			let changed = release(&tx, &a, "not_submitted")?;
			tx.commit().map_err(sqlite_error)?;
			Ok(changed)
		})
		.await
	}

	/// Runtime supplies complete guarded native history. Only the exact retained prefix proves
	/// application. An unchanged history proves non-application only after old-process death.
	pub async fn observe_chief_prompt_edit(
		&self,
		id: i64,
		generation: Option<String>,
		turns: Vec<String>,
	) -> Result<bool, StoreError> {
		self.run(move |c| {
			let tx=c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sqlite_error)?;
			let Some(r)=receipt(&tx,id)? else {return Ok(false)};
			let a=&r.attempt;
			if r.state!="reserved" || !owned(&tx,a,generation.as_deref())? {return Ok(false);}
			let same=a.generation==generation;
			if !same {
				let (Some(old),Some(new))=(&a.generation,&generation) else {return Ok(false)};
				let dead:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM process_generations old JOIN process_generation_death_evidence e ON e.evidence_id=old.death_evidence_id AND e.generation_id=old.generation_id JOIN process_generations new ON new.generation_id=?2 AND new.account_id=old.account_id WHERE old.generation_id=?1 AND old.state='dead' AND new.state='ready')",params![old,new],|r|r.get(0)).map_err(sqlite_error)?;
				if !dead {return Ok(false);}
			}
			let boundary=a.turn_ids.iter().position(|id| id==&a.before_turn_id).ok_or(StoreError::InvalidInput("invalid saved edit boundary"))?;
			let changed=if turns==a.turn_ids[..boundary] {
				let now=unix_micros()?;
				tx.execute("INSERT OR IGNORE INTO chief_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES(?1,?2,'prompt_edit_observation',?3,?4,'resolved','applied',?4)",params![format!("{}:observation",a.key()),a.work,json!({"generation":generation,"turns":turns}).to_string(),now]).map_err(sqlite_error)?==1
			} else if !same && turns==a.turn_ids { release(&tx,a,"unchanged")? } else {false};
			tx.commit().map_err(sqlite_error)?; Ok(changed)
		}).await
	}

	/// Release only after the current service has reconciled projections and handed back a draft.
	/// An applied history observation alone keeps input blocked across restart.
	pub async fn release_chief_prompt_edit_draft(
		&self,
		id: i64,
		generation: Option<String>,
	) -> Result<bool, StoreError> {
		self.run(move |c| {
			let tx = c
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(sqlite_error)?;
			let Some(r) = receipt(&tx, id)? else { return Ok(false) };
			if r.state != "applied" || !owned(&tx, &r.attempt, generation.as_deref())? {
				return Ok(false);
			}
			let changed = release(&tx, &r.attempt, "draft_restored")?;
			tx.commit().map_err(sqlite_error)?;
			Ok(changed)
		})
		.await
	}
}

#[cfg(test)] mod tests;
