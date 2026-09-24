//! Source-bound, non-waking native settings observations in the existing event journal.
use crate::{SqliteStore, StoreError, chief_process::owns_work, error::sqlite_error, unix_micros};
use rusqlite::{OptionalExtension as _, TransactionBehavior, params};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChiefPermissionObservation {
	pub id: i64,
	pub settings_json: Option<String>,
	pub source_digest: String,
}

fn latest(
	connection: &rusqlite::Connection,
	work: &str,
	thread: &str,
	generation: Option<&str>,
) -> Result<Option<ChiefPermissionObservation>, StoreError> {
	connection.query_row("SELECT id,json_extract(payload,'$.settings'),json_extract(payload,'$.sourceDigest') FROM chief_inbox_events WHERE work_item_id=?1 AND event_kind=?4 AND json_extract(payload,'$.threadId')=?2 AND json_extract(payload,'$.generationId') IS ?3 ORDER BY id DESC LIMIT 1",
		params![work,thread,generation,"native_task_permissions"], |row| Ok(ChiefPermissionObservation {id:row.get(0)?,settings_json:row.get(1)?,source_digest:row.get(2)?})).optional().map_err(|e| sqlite_error(e).into())
}

impl SqliteStore {
	async fn record_permission_observation(
		&self,
		thread: String,
		generation: Option<String>,
		settings: Option<String>,
		source_digest: String,
		publication: bool,
	) -> Result<Option<i64>, StoreError> {
		if thread.trim().is_empty()
			|| thread.len() > 512
			|| thread.chars().any(char::is_control)
			|| generation
				.as_ref()
				.is_some_and(|g| g.is_empty() || g.len() > 512 || g.chars().any(char::is_control))
			|| source_digest.len() != 64
			|| !source_digest.bytes().all(|b| b.is_ascii_hexdigit())
		{
			return Err(StoreError::InvalidInput("invalid native settings source"));
		}
		let settings = settings
			.map(|s| {
				if s.len() > 65536 {
					return Err(StoreError::InvalidInput("native settings exceed bound"));
				}
				serde_json::from_str::<Value>(&s)
					.ok()
					.filter(Value::is_object)
					.ok_or(StoreError::InvalidInput("invalid projected native settings"))
			})
			.transpose()?;
		self.run(move |connection| {
			let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sqlite_error)?;
			let (work,count): (Option<String>,i64) = tx.query_row("SELECT MIN(id),COUNT(*) FROM chief_work_items WHERE codex_thread_id=?1",[&thread],|r|Ok((r.get(0)?,r.get(1)?))).map_err(sqlite_error)?;
			let Some(work) = work.filter(|_|count==1) else { return Ok(None); };
			if !owns_work(&tx,&work,generation.as_deref())? { return Ok(None); }
			let previous = latest(&tx,&work,&thread,generation.as_deref())?;
			if previous.is_none() && settings.is_none() { return Ok(None); }
			let encoded = settings.as_ref().map(Value::to_string);
			if let Some(previous) = &previous && previous.settings_json == encoded && previous.source_digest == source_digest {
				if publication {crate::chief_permissions::observe(&tx,&work,&thread,generation.as_deref(),previous.id,settings.as_ref())?;}
				tx.commit().map_err(sqlite_error)?; return Ok(Some(previous.id));
			}
			let payload = json!({"threadId":thread,"generationId":generation,"sourceDigest":source_digest,"settings":settings});
			let identity = json!(["native_task_permissions", work, previous.as_ref().map(|p|p.id), payload]);
			let digest:String = Sha256::digest(identity.to_string().as_bytes()).iter().map(|b|format!("{b:02x}")).collect();
			let now=unix_micros()?;
			tx.execute("INSERT INTO chief_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES(?1,?2,?5,?3,?4,'resolved','native observation',?4)",params![format!("native-task-settings:{digest}"),work,payload.to_string(),now,"native_task_permissions"]).map_err(sqlite_error)?;
			let id=tx.last_insert_rowid();
			if publication {crate::chief_permissions::observe(&tx,&work,&thread,generation.as_deref(),id,settings.as_ref())?;}
			tx.commit().map_err(sqlite_error)?; Ok(Some(id))
		}).await
	}

	/// Record native permission facts without scheduling work or claiming a local write applied.
	pub async fn record_chief_task_permissions(
		&self,
		thread: String,
		generation: Option<String>,
		settings: Option<String>,
		source_digest: String,
	) -> Result<Option<i64>, StoreError> {
		self.record_permission_observation(thread, generation, settings, source_digest, false).await
	}

	/// Record permission facts verified against the current transport observation. Matching later
	/// facts can settle a reserved target; historical or stale facts must use the other method.
	pub async fn record_chief_task_permissions_publication(
		&self,
		thread: String,
		generation: Option<String>,
		settings: Option<String>,
		source_digest: String,
	) -> Result<Option<i64>, StoreError> {
		self.record_permission_observation(thread, generation, settings, source_digest, true).await
	}

	/// Read only permission observations from this exact current native owner.
	pub async fn chief_task_permissions(
		&self,
		work: String,
		thread: String,
		generation: Option<String>,
	) -> Result<Option<ChiefPermissionObservation>, StoreError> {
		self.read_permission_observation(work, thread, generation).await
	}

	async fn read_permission_observation(
		&self,
		work: String,
		thread: String,
		generation: Option<String>,
	) -> Result<Option<ChiefPermissionObservation>, StoreError> {
		self.run(move |connection| {
			let tx = connection.transaction().map_err(sqlite_error)?;
			let bound: bool = tx
				.query_row(
					"SELECT EXISTS(SELECT 1 FROM chief_work_items WHERE id=?1 AND codex_thread_id=?2)",
					params![work, thread],
					|r| r.get(0),
				)
				.map_err(sqlite_error)?;
			if !bound || !owns_work(&tx, &work, generation.as_deref())? {
				return Ok(None);
			}
			let result = latest(&tx, &work, &thread, generation.as_deref())?;
			tx.commit().map_err(sqlite_error)?;
			Ok(result)
		})
		.await
	}
}
