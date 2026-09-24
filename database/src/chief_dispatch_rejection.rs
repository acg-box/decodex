//! Preserve input after a native refusal that precedes all external effects.
use crate::{ChiefWorkItem, SqliteStore, StoreError, error::sqlite_error};
use rusqlite::params;

/// Native refusal established before any external effect.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChiefDispatchRefusal {
	/// Native connection has stopped admitting work.
	ServerDraining,
	/// Managed provider changed before native dispatch.
	ManagedProviderChanged,
	/// Native task settings changed before the transport write.
	SettingsChanged,
	/// The local request was too large to send.
	RequestTooLarge,
	/// The local request queue refused the input before forwarding it.
	RequestQueueFull,
}
impl ChiefDispatchRefusal {
	fn reason(self) -> &'static str {
		match self {
			Self::RequestTooLarge => "requestTooLarge",
			Self::RequestQueueFull => "requestQueueFull",
			Self::SettingsChanged => "settingsChanged",
			Self::ServerDraining => "serverDraining",
			Self::ManagedProviderChanged => "managedProviderChanged",
		}
	}

	pub(crate) fn note(self) -> &'static str {
		match self {
			Self::RequestTooLarge =>
				"Not sent: this input exceeds the connection size limit. Reduce the input before sending again.",
			Self::RequestQueueFull =>
				"Not sent: the local connection queue is full. Wait for pending requests before sending again.",
			Self::SettingsChanged =>
				"Not sent: task settings changed. Review the current settings and send again.",
			Self::ServerDraining =>
				"Not sent: the native server is draining. Reconnect and send again.",
			Self::ManagedProviderChanged =>
				"Not sent: managed model provider requirements changed. Restart the Codex connection before sending again.",
		}
	}
}

impl SqliteStore {
	/// Caller must prove no injection preceded the refused turn.
	pub async fn reject_chief_dispatch(
		&self,
		previous: ChiefWorkItem,
		generation: Option<String>,
		retry_event: Option<i64>,
		refusal: ChiefDispatchRefusal,
	) -> Result<(), StoreError> {
		if previous.dispatch_state != crate::ChiefDispatchState::Idle {
			return Err(StoreError::InvalidInput("invalid pre-dispatch state"));
		}
		self.run(move |connection| {
			let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(sqlite_error)?;
			let valid: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM chief_work_items WHERE id=?1 AND codex_thread_id=?2 AND dispatch_state='dispatching' AND active_turn_id IS ?3)",params![previous.id,previous.codex_thread_id,previous.active_turn_id],|row|row.get(0)).map_err(sqlite_error)?;
			if !valid || !crate::chief_process::owns_work(&tx,&previous.id,generation.as_deref())? {
				return Err(StoreError::InvalidInput("dispatch ownership changed"));
			}
			let now = crate::unix_micros()?;
			if let Some(event) = retry_event {
				let failed: String = tx.query_row("SELECT failed_turn_id FROM chief_capacity_retries WHERE event_id=?1 AND work_item_id=?2 AND state='claimed'",params![event,previous.id],|row|row.get(0)).map_err(sqlite_error)?;
				let source = serde_json::json!(["capacity_retry_rejected",event]).to_string();
				let payload = serde_json::json!({"retryEventId":event,"reason":refusal.reason()}).to_string();
				tx.execute("INSERT INTO chief_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES(?1,?2,'capacity_retry_rejected',?3,?4,'resolved',?5,?4)",params![source,previous.id,payload,now,refusal.note()]).map_err(sqlite_error)?;
				tx.execute("UPDATE chief_capacity_retries SET state='cancelled' WHERE event_id=?1",[event]).map_err(sqlite_error)?;
				// The original input DID reach the failed turn. Restore that receipt.
				tx.execute("UPDATE chief_inbox_events SET delivered_turn_id=?2 WHERE delivery_work_item_id=?1 AND delivered_turn_id='' AND disposition IS NULL",params![previous.id,failed]).map_err(sqlite_error)?;
				if previous.parent_goal_id.is_some() {
					let original: String = tx.query_row("SELECT payload FROM chief_inbox_events WHERE id=?1",[event],|row|row.get(0)).map_err(sqlite_error)?;
					let mut payload: serde_json::Value = serde_json::from_str(&original).map_err(|_|StoreError::InvalidInput("invalid capacity receipt"))?;
					payload["capacityRetry"]["cancelled"] = serde_json::json!(true);
					tx.execute("INSERT INTO chief_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros) VALUES(?1,?2,'worker_turn_completed',?3,?4)",params![format!("capacity-refused:{event}"),previous.id,payload.to_string(),now]).map_err(sqlite_error)?;
				}
			} else {
			let unsafe_claim: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM chief_inbox_events WHERE delivery_work_item_id=?1 AND delivered_turn_id='' AND disposition IS NULL AND event_kind NOT IN ('user_message','async_question_answer','work_instruction')) OR EXISTS(SELECT 1 FROM chief_capacity_retries WHERE work_item_id=?1 AND state='claimed')",[&previous.id],|row|row.get(0)).map_err(sqlite_error)?;
			if unsafe_claim { return Err(StoreError::InvalidInput("dispatch has other effects")); }
			tx.execute("UPDATE chief_inbox_events SET disposition='user_decision',disposition_note=?3,disposed_at_micros=max(created_at_micros,?2),delivery_work_item_id=NULL,delivered_turn_id=NULL WHERE delivery_work_item_id=?1 AND delivered_turn_id='' AND disposition IS NULL",params![previous.id,now,refusal.note()]).map_err(sqlite_error)?;
			}
			tx.execute("UPDATE chief_work_items SET dispatch_state='idle',status='user_decision',next_check_at_micros=NULL,updated_at_micros=max(updated_at_micros,?2) WHERE id=?1",params![previous.id,now]).map_err(sqlite_error)?;
			// A rejected ordinary prompt may have retired question projections locally.
			tx.execute("INSERT INTO chief_async_recovery(work_id,thread_id,required_item_id) VALUES(?1,?2,NULL) ON CONFLICT(work_id,thread_id) DO UPDATE SET required_item_id=NULL",params![previous.id,previous.codex_thread_id]).map_err(sqlite_error)?;
			tx.commit().map_err(sqlite_error)?;
			Ok(())
		}).await
	}
}
