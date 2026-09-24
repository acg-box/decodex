//! Shared native hook config receipts. Task ownership authorizes a write, not its shared scope.
use crate::{
	SqliteStore, StoreError,
	chief_config_journal::{available, dead, digest, owned, text},
	error::sqlite_error,
	unix_micros,
};
use rusqlite::{OptionalExtension as _, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

pub type ChiefHookOwner = crate::ChiefConfigOwner;
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ChiefHookAttempt {
	pub owner: ChiefHookOwner,
	/// Digest of the native writable config file, without account or task partitioning.
	pub scope: String,
	pub hook: String,
	pub field: String,
	pub value: Value,
	/// Raw reviewed override; a no-op must not create an uncertain write.
	pub previous_value: Option<Value>,
	pub config_version: String,
	pub review_token: String,
	pub attempt_id: String,
	pub previous_id: Option<i64>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChiefHookReceipt {
	pub id: i64,
	pub attempt: ChiefHookAttempt,
	pub state: String,
}

pub struct ChiefHookObservation {
	pub owner: ChiefHookOwner,
	pub scope: String,
	pub hook: String,
	pub field: String,
	/// Raw override in the writable native layer. None means the field is absent, not disabled.
	pub value: Option<Value>,
	pub config_version: String,
}
fn target(field: &str, value: &Value) -> bool {
	match field {
		"enabled" => value.is_boolean(),
		"trusted_hash" => value.as_str().is_some_and(text),
		_ => false,
	}
}
pub(crate) fn latest(
	c: &rusqlite::Connection,
	scope: &str,
) -> Result<Option<ChiefHookReceipt>, StoreError> {
	let row:Option<(i64,String,String)>=c.query_row("SELECT a.id,a.payload,COALESCE(o.disposition_note,r.disposition_note,'reserved') FROM chief_inbox_events a LEFT JOIN chief_inbox_events r ON r.source_event_id='hook-result:'||a.id AND r.event_kind='hook_setting_result' LEFT JOIN chief_inbox_events o ON o.source_event_id='hook-observation:'||a.id AND o.event_kind='hook_setting_observation' WHERE a.event_kind='hook_setting_attempt' AND json_extract(a.payload,'$.scope')=?1 ORDER BY a.id DESC LIMIT 1",[scope],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(sqlite_error)?;
	row.map(|(id, payload, state)| {
		Ok(ChiefHookReceipt {
			id,
			attempt: serde_json::from_str(&payload)
				.map_err(|_| StoreError::InvalidInput("invalid saved hook receipt"))?,
			state,
		})
	})
	.transpose()
}
fn unresolved(state: &str) -> bool {
	matches!(state, "reserved" | "unknown")
}
impl SqliteStore {
	/// Read the latest receipt for the entire writable config, across tasks and accounts.
	pub async fn chief_hook_receipt(
		&self,
		scope: String,
	) -> Result<Option<ChiefHookReceipt>, StoreError> {
		self.run(move |c| latest(c, &scope)).await
	}

	/// Reserve one reviewed shared edit. An unresolved write excludes other tasks using that file.
	pub async fn reserve_chief_hook_setting(
		&self,
		a: ChiefHookAttempt,
	) -> Result<Option<i64>, StoreError> {
		if ![
			&a.owner.work,
			&a.owner.thread,
			&a.owner.generation,
			&a.owner.account,
			&a.hook,
			&a.config_version,
			&a.attempt_id,
		]
		.iter()
		.all(|v| text(v))
			|| !digest(&a.scope)
			|| !digest(&a.review_token)
			|| !target(&a.field, &a.value)
			|| a.previous_value.as_ref() == Some(&a.value)
			|| a.previous_value.as_ref().is_some_and(|v| !target(&a.field, v))
			|| a.previous_id.is_some_and(|id| id <= 0)
		{
			return Err(StoreError::InvalidInput("invalid hook attempt"));
		}
		self.run(move|c|{
   let tx=c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sqlite_error)?;
   if !owned(&tx,&a.owner)? {return Err(StoreError::OwnershipLost("hook configuration owner"));}
   let prior=latest(&tx,&a.scope)?;
   if prior.as_ref().map(|r|r.id)!=a.previous_id || !available(&tx,&a.scope,&a.review_token)? {return Ok(None);}
   let hash:String=Sha256::digest(json!([a.scope,a.review_token]).to_string().as_bytes()).iter().map(|b|format!("{b:02x}")).collect();
   let source=format!("hook-attempt:{hash}");
   let now=unix_micros()?;
   let inserted=tx.execute("INSERT OR IGNORE INTO chief_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES(?1,?2,'hook_setting_attempt',?3,?4,'resolved','reserved',?4)",params![source,a.owner.work,serde_json::to_string(&a).expect("hook attempt"),now]).map_err(sqlite_error)?;
   let id=(inserted==1).then(||tx.last_insert_rowid());tx.commit().map_err(sqlite_error)?;Ok(id)
  }).await
	}

	/// Retain the original response once. A save is not proof of effective hook behavior.
	pub async fn finish_chief_hook_setting(
		&self,
		id: i64,
		attempt: String,
		state: String,
	) -> Result<bool, StoreError> {
		if !matches!(state.as_str(), "saved" | "overridden" | "rejected" | "unknown") {
			return Err(StoreError::InvalidInput("invalid hook result"));
		}
		self.run(move|c|Ok(c.execute("INSERT OR IGNORE INTO chief_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) SELECT 'hook-result:'||id,work_item_id,'hook_setting_result',json_object('reservation',id),?4,'resolved',?3,?4 FROM chief_inbox_events WHERE id=?1 AND event_kind='hook_setting_attempt' AND json_extract(payload,'$.attempt_id')=?2 AND NOT EXISTS(SELECT 1 FROM chief_inbox_events o WHERE o.source_event_id='hook-observation:'||?1)",params![id,attempt,state,unix_micros()?]).map_err(sqlite_error)?==1)).await
	}

	/// Reconcile an unresolved write from a current source's raw config read, never by replay.
	/// A different source must prove the original process dead before settling the old attempt.
	pub async fn observe_chief_hook_setting(
		&self,
		id: i64,
		o: ChiefHookObservation,
	) -> Result<bool, StoreError> {
		if !digest(&o.scope)
			|| !text(&o.config_version)
			|| !text(&o.hook)
			|| !matches!(o.field.as_str(), "enabled" | "trusted_hash")
			|| o.value.as_ref().is_some_and(|v| !target(&o.field, v))
		{
			return Err(StoreError::InvalidInput("invalid hook observation"));
		}
		self.run(move|c|{
   let tx=c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sqlite_error)?;
   if !owned(&tx,&o.owner)? {return Ok(false);}
   let Some(prior)=latest(&tx,&o.scope)? else {return Ok(false)};
   let a=&prior.attempt;
   if prior.id!=id || !unresolved(&prior.state) || a.hook!=o.hook || a.field!=o.field {return Ok(false);}
   let same=a.owner.generation==o.owner.generation;
   let matches=o.value.as_ref()==Some(&a.value);
   if same && (!matches || a.config_version==o.config_version) {return Ok(false);}
   if !same && !dead(&tx,&a.owner.generation)? {return Ok(false);}
   let state=if matches {"target_observed"} else {"superseded"};
   let payload=json!({"reservation":id,"observer":o.owner,"configVersion":o.config_version,"value":o.value});let now=unix_micros()?;
   let changed=tx.execute("INSERT OR IGNORE INTO chief_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES(?1,?2,'hook_setting_observation',?3,?4,'resolved',?5,?4)",params![format!("hook-observation:{id}"),a.owner.work,payload.to_string(),now,state]).map_err(sqlite_error)?;
   tx.commit().map_err(sqlite_error)?;Ok(changed==1)
  }).await
	}
}
