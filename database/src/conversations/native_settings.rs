//! Source-bound native response observations, separate from requested execution settings.
use super::sql_error;
use crate::{SqliteStore, StoreError, unix_micros};
use decodex_core::{ProcessGenerationId, RuntimeSessionId};
use rusqlite::{OptionalExtension as _, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationNativeSettings {
	pub model: String,
	pub model_provider: String,
	pub cwd: String,
	pub reasoning_effort: Option<String>,
}
impl ConversationNativeSettings {
	fn valid(&self) -> bool {
		fn label(value: &str, limit: usize) -> bool {
			!value.trim().is_empty() && value.len() <= limit && !value.chars().any(char::is_control)
		}
		label(&self.model, 128)
			&& label(&self.model_provider, 512)
			&& label(&self.cwd, 4096)
			&& self.cwd.starts_with('/')
			&& self.reasoning_effort.as_ref().is_none_or(|value| label(value, 128))
	}
}

#[derive(Clone, Debug)]
pub struct RecordConversationNativeSettings {
	pub runtime_session_id: RuntimeSessionId,
	pub expected_session_revision: i64,
	pub codex_thread_id: String,
	pub process_generation_id: ProcessGenerationId,
	pub expected_process_revision: i64,
	pub response_id: i64,
	pub response_sha256: String,
	pub settings: ConversationNativeSettings,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConversationNativeSettingsObservation {
	pub settings: ConversationNativeSettings,
	pub observed_at_micros: i64,
	pub process_generation_id: String,
	pub account_id: String,
	pub account_revision: i64,
}

impl SqliteStore {
	/// Retain a native response only while its exact process/session/account source owns it.
	/// Late responses cannot replace newer facts. A replacement process needs confirmed old death.
	pub async fn record_conversation_native_settings(
		&self,
		input: &RecordConversationNativeSettings,
	) -> Result<bool, StoreError> {
		if input.expected_session_revision <= 0
			|| input.expected_process_revision <= 0
			|| input.response_id <= 0
			|| input.codex_thread_id.is_empty()
			|| input.codex_thread_id.len() > 512
			|| input.response_sha256.len() != 64
			|| !input.response_sha256.bytes().all(|b| b.is_ascii_hexdigit())
			|| !input.settings.valid()
		{
			return Err(StoreError::InvalidInput("invalid native conversation settings"));
		}
		let input = input.clone();
		let settings = serde_json::to_string(&input.settings)
			.map_err(|_| StoreError::InvalidInput("invalid native settings encoding"))?;
		self.run(move |connection| {
            let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sql_error)?;
            let owner: Option<(String,i64,String)> = tx.query_row(
                "SELECT p.account_id,p.account_revision,s.conversation_id FROM runtime_sessions s
                 JOIN conversations c USING(conversation_id)
                 JOIN process_generations p ON p.runtime_session_id=s.runtime_session_id
                 JOIN accounts a ON a.account_id=p.account_id
                 WHERE s.runtime_session_id=?1 AND s.revision=?2 AND s.codex_thread_id=?3 AND s.state='active'
                   AND c.state='active' AND c.kind='ordinary_task'
                   AND p.generation_id=?4 AND p.revision=?5 AND p.state='ready'
                   AND p.account_id=s.account_id AND a.revision=p.account_revision",
                params![input.runtime_session_id.as_str(),input.expected_session_revision,input.codex_thread_id,input.process_generation_id.as_str(),input.expected_process_revision],
                |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
            ).optional().map_err(sql_error)?;
            let Some((account,revision,conversation)) = owner else { return Ok(false); };
            let prior: Option<(String,i64,String,String,i64)> = tx.query_row(
                "SELECT process_generation_id,response_id,response_sha256,settings_json,observed_at_micros FROM conversation_native_settings WHERE runtime_session_id=?1",
                [input.runtime_session_id.as_str()], |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)),
            ).optional().map_err(sql_error)?;
            if let Some((generation,id,digest,json,_)) = &prior {
                if generation == input.process_generation_id.as_str() {
                    if *id >= input.response_id {
                        return Ok(*id == input.response_id && digest == &input.response_sha256 && json == &settings);
                    }
                } else {
                    let dead: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM process_generations p JOIN process_generation_death_evidence d ON d.generation_id=p.generation_id AND d.evidence_id=p.death_evidence_id WHERE p.generation_id=?1 AND p.state='dead')", [generation], |r|r.get(0)).map_err(sql_error)?;
                    if !dead { return Ok(false); }
                }
            }
            let now = unix_micros().map_err(StoreError::from)?.max(prior.as_ref().map_or(0,|p|p.4.saturating_add(1)));
            tx.execute("INSERT INTO conversation_native_settings(runtime_session_id,codex_thread_id,process_generation_id,process_generation_revision,account_id,account_revision,response_id,response_sha256,settings_json,observed_at_micros)
                VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
                ON CONFLICT(runtime_session_id) DO UPDATE SET codex_thread_id=excluded.codex_thread_id,process_generation_id=excluded.process_generation_id,process_generation_revision=excluded.process_generation_revision,account_id=excluded.account_id,account_revision=excluded.account_revision,response_id=excluded.response_id,response_sha256=excluded.response_sha256,settings_json=excluded.settings_json,observed_at_micros=excluded.observed_at_micros",
                params![input.runtime_session_id.as_str(),input.codex_thread_id,input.process_generation_id.as_str(),input.expected_process_revision,account,revision,input.response_id,input.response_sha256,settings,now]).map_err(sql_error)?;
            tx.execute("UPDATE conversations SET updated_at_micros=MAX(updated_at_micros+1,?2) WHERE conversation_id=?1", params![conversation,now]).map_err(sql_error)?;
            tx.commit().map_err(sql_error)?;
            Ok(true)
        }).await
	}

	/// Last observation for this still-bound session/thread. It is not live execution authority.
	pub async fn conversation_native_settings(
		&self,
		session: RuntimeSessionId,
		thread: String,
	) -> Result<Option<ConversationNativeSettingsObservation>, StoreError> {
		self.run(move |connection| read(connection, session.as_str(), &thread)).await
	}
}

pub(super) fn read(
	connection: &rusqlite::Connection,
	session: &str,
	thread: &str,
) -> Result<Option<ConversationNativeSettingsObservation>, StoreError> {
	let row: Option<(String,i64,String,String,i64)> = connection.query_row(
        "SELECT n.settings_json,n.observed_at_micros,n.process_generation_id,n.account_id,n.account_revision
         FROM conversation_native_settings n JOIN runtime_sessions s USING(runtime_session_id)
         WHERE n.runtime_session_id=?1 AND n.codex_thread_id=?2 AND s.codex_thread_id=n.codex_thread_id AND s.account_id=n.account_id",
        params![session,thread],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)),
    ).optional().map_err(sql_error)?;
	row.map(|(json, time, generation, account, revision)| {
		let settings: ConversationNativeSettings = serde_json::from_str(&json)
			.map_err(|_| StoreError::InvalidInput("stored native settings are invalid"))?;
		if !settings.valid() {
			return Err(StoreError::InvalidInput("stored native settings exceed bounds"));
		}
		Ok(ConversationNativeSettingsObservation {
			settings,
			observed_at_micros: time,
			process_generation_id: generation,
			account_id: account,
			account_revision: revision,
		})
	})
	.transpose()
}
