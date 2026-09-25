//! Durable app-link configuration writes. Saving a setting never answers its pending request.
use crate::{
	ChiefConfigOwner, SqliteStore, StoreError,
	chief_config_journal::{available, dead, digest, owned, text},
	error::sqlite_error,
	unix_micros,
};
use rusqlite::{OptionalExtension as _, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ChiefAppSettingsAttempt {
	pub owner: ChiefConfigOwner,
	/// Originating pending request, or None for an explicitly reviewed saved native override.
	pub request_event_id: Option<i64>,
	/// Digest of the native writable config file, without account or task partitioning.
	pub scope: String,
	pub connector: String,
	/// Empty only for connector-level omit_tools_from; connection edits require a link.
	pub link: String,
	pub field: String,
	pub value: Option<Value>,
	/// Raw reviewed override; a no-op must not create an uncertain write.
	pub previous_value: Option<Value>,
	pub config_version: String,
	pub review_token: String,
	pub attempt_id: String,
	pub previous_id: Option<i64>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChiefAppSettingsReceipt {
	pub id: i64,
	pub attempt: ChiefAppSettingsAttempt,
	pub state: String,
	pub saved_version: Option<String>,
}

pub struct ChiefAppSettingsObservation {
	pub owner: ChiefConfigOwner,
	pub scope: String,
	pub connector: String,
	/// Empty only for connector-level omit_tools_from; connection edits require a link.
	pub link: String,
	pub field: String,
	/// Raw override in the writable native layer. None means the field is absent, not disabled.
	pub value: Option<Value>,
	pub config_version: String,
}
fn address(field: &str, link: &str) -> bool {
	match field {
		"omit_tools_from" => link.is_empty(),
		"default_tools_approval_mode" | "approvals_reviewer" => text(link),
		_ => false,
	}
}
fn raw_value(field: &str, value: &Value) -> bool {
	if field == "omit_tools_from" {
		value
			.as_array()
			.is_some_and(|a| a.len() <= 16 && a.iter().all(|v| v.as_str().is_some_and(text)))
	} else {
		value.as_str().is_some_and(text)
	}
}
fn target(field: &str, value: &Value) -> bool {
	match field {
		"omit_tools_from" => value.as_array().is_some_and(|a| {
			a.len() <= 3
				&& a.iter().all(|v| {
					v.as_str().is_some_and(|s| matches!(s, "code_mode" | "deferred" | "direct"))
				}) && a.iter().collect::<std::collections::HashSet<_>>().len() == a.len()
		}),
		"default_tools_approval_mode" =>
			value.as_str().is_some_and(|v| matches!(v, "auto" | "prompt" | "writes" | "approve")),
		"approvals_reviewer" => value.as_str().is_some_and(|v| matches!(v, "user" | "auto_review")),
		_ => false,
	}
}
pub(crate) fn latest(
	c: &rusqlite::Connection,
	scope: &str,
) -> Result<Option<ChiefAppSettingsReceipt>, StoreError> {
	let row:Option<(i64,String,String,Option<String>)>=c.query_row("SELECT a.id,a.payload,COALESCE(o.disposition_note,r.disposition_note,'reserved'),json_extract(r.payload,'$.version') FROM chief_inbox_events a LEFT JOIN chief_inbox_events r ON r.source_event_id='app-result:'||a.id AND r.event_kind='app_setting_result' LEFT JOIN chief_inbox_events o ON o.source_event_id='app-observation:'||a.id AND o.event_kind='app_setting_observation' WHERE a.event_kind='app_setting_attempt' AND json_extract(a.payload,'$.scope')=?1 ORDER BY a.id DESC LIMIT 1",[scope],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(sqlite_error)?;
	row.map(|(id, payload, state, saved_version)| {
		Ok(ChiefAppSettingsReceipt {
			id,
			attempt: serde_json::from_str(&payload)
				.map_err(|_| StoreError::InvalidInput("invalid saved app settings receipt"))?,
			state,
			saved_version,
		})
	})
	.transpose()
}
fn unresolved(state: &str) -> bool {
	matches!(state, "reserved" | "unknown")
}
impl SqliteStore {
	/// Read the latest receipt for the entire writable config, across tasks and accounts.
	pub async fn chief_app_settings_receipt(
		&self,
		scope: String,
	) -> Result<Option<ChiefAppSettingsReceipt>, StoreError> {
		self.run(move |c| latest(c, &scope)).await
	}

	/// Reserve one reviewed shared edit. An unresolved write excludes other tasks using that file.
	/// With a request, the service must guard its exact native identity and verify child ownership.
	/// Without a request, it must review an existing saved native connection override and guard
	/// the current source. The journal does not infer native config contents from task events.
	/// The originating turn may have yielded; it need not be the currently active turn.
	pub async fn reserve_chief_app_settings_attempt(
		&self,
		a: ChiefAppSettingsAttempt,
	) -> Result<Option<i64>, StoreError> {
		if ![
			&a.owner.work,
			&a.owner.thread,
			&a.owner.generation,
			&a.owner.account,
			&a.connector,
			&a.config_version,
			&a.attempt_id,
		]
		.iter()
		.all(|v| text(v))
			|| !digest(&a.scope)
			|| !digest(&a.review_token)
			|| a.request_event_id.is_some_and(|id| id <= 0)
			|| !address(&a.field, &a.link)
			|| (a.field == "omit_tools_from"
				&& (a.request_event_id.is_some() || a.connector == "_default"))
			|| a.value.as_ref().is_some_and(|v| !target(&a.field, v))
			|| a.previous_value == a.value
			|| a.previous_value.as_ref().is_some_and(|v| !raw_value(&a.field, v))
			|| a.previous_id.is_some_and(|id| id <= 0)
		{
			return Err(StoreError::InvalidInput("invalid app settings attempt"));
		}
		self.run(move|c|{
   let tx=c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sqlite_error)?;
   if !owned(&tx,&a.owner)? || !pending_request(&tx,&a)? {return Err(StoreError::OwnershipLost("app settings owner"));}
   let prior=latest(&tx,&a.scope)?;
   if prior.as_ref().map(|r|r.id)!=a.previous_id || !available(&tx,&a.scope,&a.review_token)? {return Ok(None);}
   let hash:String=Sha256::digest(json!([a.scope,a.review_token]).to_string().as_bytes()).iter().map(|b|format!("{b:02x}")).collect();
   let source=format!("app-attempt:{hash}");
   let now=unix_micros()?;
   let inserted=tx.execute("INSERT OR IGNORE INTO chief_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES(?1,?2,'app_setting_attempt',?3,?4,'resolved','reserved',?4)",params![source,a.owner.work,serde_json::to_string(&a).expect("app settings attempt"),now]).map_err(sqlite_error)?;
   let id=(inserted==1).then(||tx.last_insert_rowid());tx.commit().map_err(sqlite_error)?;Ok(id)
  }).await
	}

	/// Retain the original response once. A save is not proof of effective app policy.
	pub async fn finish_chief_app_settings_attempt(
		&self,
		id: i64,
		attempt: String,
		state: String,
		version: Option<String>,
	) -> Result<bool, StoreError> {
		if !matches!(state.as_str(), "saved" | "overridden" | "rejected" | "unknown")
			|| matches!(state.as_str(), "saved" | "overridden") != version.is_some()
			|| version.as_deref().is_some_and(|v| !text(v))
		{
			return Err(StoreError::InvalidInput("invalid app settings result"));
		}
		self.run(move|c|Ok(c.execute("INSERT OR IGNORE INTO chief_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) SELECT 'app-result:'||id,work_item_id,'app_setting_result',json_object('reservation',id,'version',?5),?4,'resolved',?3,?4 FROM chief_inbox_events WHERE id=?1 AND event_kind='app_setting_attempt' AND json_extract(payload,'$.attempt_id')=?2 AND NOT EXISTS(SELECT 1 FROM chief_inbox_events o WHERE o.source_event_id='app-observation:'||?1)",params![id,attempt,state,unix_micros()?,version]).map_err(sqlite_error)?==1)).await
	}

	/// Reconcile an unresolved write from a current source's raw config read, never by replay.
	/// A different source must prove the original process dead before settling the old attempt.
	pub async fn observe_chief_app_settings(
		&self,
		id: i64,
		o: ChiefAppSettingsObservation,
	) -> Result<bool, StoreError> {
		if !digest(&o.scope)
			|| !text(&o.config_version)
			|| !text(&o.connector)
			|| !address(&o.field, &o.link)
			|| o.value.as_ref().is_some_and(|v| !raw_value(&o.field, v))
		{
			return Err(StoreError::InvalidInput("invalid app settings observation"));
		}
		self.run(move|c|{
   let tx=c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sqlite_error)?;
   if !owned(&tx,&o.owner)? {return Ok(false);}
   let Some(prior)=latest(&tx,&o.scope)? else {return Ok(false)};
   let a=&prior.attempt;
   if prior.id!=id || !unresolved(&prior.state) || a.connector!=o.connector || a.link!=o.link || a.field!=o.field {return Ok(false);}
   let same=a.owner.generation==o.owner.generation;
   let matches=o.value==a.value;
   if same && (!matches || a.config_version==o.config_version) {return Ok(false);}
   if !same && !dead(&tx,&a.owner.generation)? {return Ok(false);}
   let state=if matches {"target_observed"} else {"superseded"};
   let payload=json!({"reservation":id,"observer":o.owner,"configVersion":o.config_version,"value":o.value});let now=unix_micros()?;
   let changed=tx.execute("INSERT OR IGNORE INTO chief_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES(?1,?2,'app_setting_observation',?3,?4,'resolved',?5,?4)",params![format!("app-observation:{id}"),a.owner.work,payload.to_string(),now,state]).map_err(sqlite_error)?;
   tx.commit().map_err(sqlite_error)?;Ok(changed==1)
  }).await
	}
}

fn pending_request(
	c: &rusqlite::Connection,
	a: &ChiefAppSettingsAttempt,
) -> Result<bool, StoreError> {
	let Some(event_id) = a.request_event_id else { return Ok(true) };
	let payload: Option<String> = c.query_row(
        "SELECT e.payload FROM chief_inbox_events e WHERE e.id=?1 AND e.work_item_id=?2 AND e.event_kind='server_request_pending' AND e.disposition IS NULL AND NOT EXISTS(SELECT 1 FROM chief_misalignment WHERE work_id=?2 AND thread_id=?3)",
        params![event_id,a.owner.work,a.owner.thread], |r|r.get(0)).optional().map_err(sqlite_error)?;
	Ok(payload.as_deref().and_then(|p| serde_json::from_str::<Value>(p).ok()).is_some_and(|v| {
		v["method"] == "mcpServer/elicitation/request"
			&& v["params"]["serverName"] == "codex_apps"
			&& (v["params"]["threadId"] == a.owner.thread
				|| (v["ownerThreadId"] == a.owner.thread
					&& v["params"]["threadId"].as_str().is_some_and(text)))
			&& v["params"]["_meta"]["connector_id"] == a.connector
			&& v["params"]["_meta"]["link_id"] == a.link
	}))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ChiefConfigReceipt {
	Hook(crate::ChiefHookReceipt),
	App(ChiefAppSettingsReceipt),
}
impl SqliteStore {
	/// Read the latest writer to this native file, including edits from another settings surface.
	pub async fn chief_config_receipt(
		&self,
		scope: String,
	) -> Result<Option<ChiefConfigReceipt>, StoreError> {
		self.run(move |c| {
			let tx = c.transaction().map_err(sqlite_error)?;
			let Some((_, kind)) = crate::chief_config_journal::latest_identity(&tx, &scope)? else {
				return Ok(None);
			};
			if kind == "hook_setting_attempt" {
				crate::chief_hooks::latest(&tx, &scope).map(|r| r.map(ChiefConfigReceipt::Hook))
			} else {
				latest(&tx, &scope).map(|r| r.map(ChiefConfigReceipt::App))
			}
		})
		.await
	}
}
