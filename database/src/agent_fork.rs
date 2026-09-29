//! One durable native fork intent and its independent local work owner.

use crate::{
	AgentPromptEditAttempt, SqliteStore, StoreError, agent::read_work, agent_process::owns_work,
	error::sqlite_error, unix_micros,
};
use rusqlite::{Connection, OptionalExtension as _, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentForkBoundary {
	BeforeInput,
	AfterTurn,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentForkAttempt {
	pub source: AgentPromptEditAttempt,
	pub target_work: String,
	pub boundary: AgentForkBoundary,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentForkReceipt {
	pub id: i64,
	pub attempt: AgentForkAttempt,
	/// reserved, acknowledged, forked, or rejected. Reserved never grants permission to repeat the
	/// RPC.
	pub state: String,
	pub target_thread: Option<String>,
	/// Before-input forks reuse the canonical prompt draft and upload owner.
	pub edit_receipt_id: Option<i64>,
}

impl AgentForkAttempt {
	fn key(&self) -> String {
		format!("thread-fork:{}:{}", self.source.work, self.source.review_token)
	}

	fn validate(&self) -> Result<(), StoreError> {
		self.source.validate()?;
		if self.target_work.is_empty()
			|| self.target_work.len() > 512
			|| self.target_work.chars().any(char::is_control)
			|| self.target_work == self.source.work
		{
			return Err(StoreError::InvalidInput("invalid fork target"));
		}
		Ok(())
	}

	pub fn expected_turns(&self) -> Option<&[String]> {
		let boundary =
			self.source.turn_ids.iter().position(|turn| turn == &self.source.before_turn_id)?;
		let end = boundary + usize::from(self.boundary == AgentForkBoundary::AfterTurn);
		Some(&self.source.turn_ids[..end])
	}
}

fn receipt(c: &Connection, key: &str) -> Result<Option<AgentForkReceipt>, StoreError> {
	let saved: Option<(i64,String,String,Option<String>)> = c.query_row(
		"SELECT a.id,a.payload,coalesce(o.disposition_note,k.disposition_note,'reserved'),coalesce(o.payload,k.payload) FROM agent_inbox_events a LEFT JOIN agent_inbox_events k ON k.source_event_id=a.source_event_id||':acknowledgement' AND k.event_kind='thread_fork_identity' LEFT JOIN agent_inbox_events o ON o.source_event_id=a.source_event_id||':observation' AND o.event_kind='thread_fork_observation' WHERE a.source_event_id=?1 AND a.event_kind='thread_fork_attempt'",
		[key], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)),
	).optional().map_err(sqlite_error)?;
	saved
		.map(|(id, payload, state, observed)| {
			let attempt: AgentForkAttempt = serde_json::from_str(&payload)
				.map_err(|_| StoreError::InvalidInput("invalid saved fork"))?;
			attempt.validate()?;
			let observed: serde_json::Value = observed
				.map(|value| serde_json::from_str(&value))
				.transpose()
				.map_err(|_| StoreError::InvalidInput("invalid fork observation"))?
				.unwrap_or_default();
			Ok(AgentForkReceipt {
				id,
				attempt,
				state,
				target_thread: observed["thread"].as_str().map(str::to_owned),
				edit_receipt_id: observed["edit_receipt_id"].as_i64(),
			})
		})
		.transpose()
}

impl SqliteStore {
	/// Retain the native acknowledgement before a separate prefix read can fail.
	pub async fn record_agent_fork_identity(
		&self,
		attempt: AgentForkAttempt,
		target_thread: String,
	) -> Result<Option<AgentForkReceipt>, StoreError> {
		attempt.validate()?;
		if target_thread.is_empty()
			|| target_thread.len() > 512
			|| target_thread.chars().any(char::is_control)
			|| target_thread == attempt.source.thread
		{
			return Err(StoreError::InvalidInput("invalid fork identity"));
		}
		self.run(move |c| {
			let tx=c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sqlite_error)?;
			let Some(saved)=receipt(&tx,&attempt.key())? else { return Ok(None); };
			if saved.attempt != attempt { return Ok(None); }
			if matches!(saved.state.as_str(), "acknowledged" | "forked") { return Ok((saved.target_thread.as_deref()==Some(target_thread.as_str())).then_some(saved)); }
			if saved.state != "reserved" { return Ok(None); }
			let now=unix_micros()?;
			tx.execute("INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES(?1,?2,'thread_fork_identity',?3,?4,'resolved','acknowledged',?4)",params![format!("{}:acknowledgement",attempt.key()),attempt.source.work,json!({"thread":target_thread}).to_string(),now]).map_err(sqlite_error)?;
			let saved=receipt(&tx,&attempt.key())?;
			tx.commit().map_err(sqlite_error)?;
			Ok(saved)
		}).await
	}

	/// Record a proved pre-write or invalid-request refusal. Never use this for timeouts.
	pub async fn reject_agent_fork(&self, attempt: AgentForkAttempt) -> Result<bool, StoreError> {
		attempt.validate()?;
		self.run(move |c| {
			let tx=c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sqlite_error)?;
			let Some(saved)=receipt(&tx,&attempt.key())? else { return Ok(false); };
			if saved.attempt != attempt || saved.state != "reserved" { return Ok(false); }
			let now=unix_micros()?;
			let changed=tx.execute("UPDATE agent_work_items SET status='resolved',dispatch_state='idle',updated_at_micros=max(updated_at_micros,?2) WHERE id=?1 AND codex_thread_id IS NULL AND dispatch_state='dispatching'",params![attempt.target_work,now]).map_err(sqlite_error)?;
			if changed!=1 { return Ok(false); }
			tx.execute("INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES(?1,?2,'thread_fork_observation','{}',?3,'resolved','rejected',?3)",params![format!("{}:observation",attempt.key()),attempt.source.work,now]).map_err(sqlite_error)?;
			tx.commit().map_err(sqlite_error)?;
			Ok(true)
		}).await
	}

	/// Reserve a new sibling (or a child of Main) before one native fork request.
	/// Source history and source input eligibility remain unchanged.
	pub async fn reserve_agent_fork(
		&self,
		attempt: AgentForkAttempt,
	) -> Result<Option<AgentForkReceipt>, StoreError> {
		attempt.validate()?;
		self.run(move |c| {
			let tx = c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sqlite_error)?;
			if receipt(&tx, &attempt.key())?.is_some() { return Ok(None); }
			let a=&attempt.source;
			let source = read_work(&tx, &a.work)?;
			if !owns_work(&tx, &a.work, a.generation.as_deref())?
				|| source.codex_thread_id.as_deref() != Some(a.thread.as_str())
				|| source.dispatch_state != crate::AgentDispatchState::Idle
				|| source.active_turn_id.is_some() || source.status == crate::AgentWorkStatus::Resolved
				|| crate::agent_prompt_edit::pending(&tx, &a.work)?
			{ return Ok(None); }
			let mut title = format!("{} (branch)", source.title);
			while title.len() > 1024 { title.pop(); }
			let parent = source.parent_goal_id.as_deref().unwrap_or(&source.id);
			let now=unix_micros()?;
			tx.execute("INSERT INTO agent_work_items(id,parent_goal_id,kind,title,instructions,status,dispatch_state,created_at_micros,updated_at_micros) VALUES(?1,?2,?3,?4,?5,'open','dispatching',?6,?6)", params![attempt.target_work,parent,source.kind.as_str(),title,source.instructions,now]).map_err(sqlite_error)?;
			// Preserve the source tool version; a fork is not a tool-upgrade mechanism.
			tx.execute("INSERT INTO agent_tool_versions(work_id,version) SELECT ?1,version FROM agent_tool_versions WHERE work_id=?2", params![attempt.target_work,a.work]).map_err(sqlite_error)?;
			let manager: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_managers WHERE work_id=?1) OR EXISTS(SELECT 1 FROM agent_work_items WHERE id=?1 AND parent_goal_id IS NULL)", [&a.work], |r|r.get(0)).map_err(sqlite_error)?;
			if manager {
				tx.execute("INSERT INTO agent_managers(work_id) VALUES(?1)", [&attempt.target_work]).map_err(sqlite_error)?;
				tx.execute("INSERT INTO agent_workspaces(agent_id,name,directory) SELECT ?1,name,directory FROM agent_workspaces WHERE agent_id=?2", params![attempt.target_work,a.work]).map_err(sqlite_error)?;
			}
			tx.execute("INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES(?1,?2,'thread_fork_attempt',?3,?4,'resolved','reserved',?4)", params![attempt.key(),a.work,json!(attempt).to_string(),now]).map_err(sqlite_error)?;
			let saved=receipt(&tx,&attempt.key())?;
			tx.commit().map_err(sqlite_error)?;
			Ok(saved)
		}).await
	}

	pub async fn agent_fork_receipt(
		&self,
		source_work: String,
		review: String,
	) -> Result<Option<AgentForkReceipt>, StoreError> {
		self.run(move |c| receipt(c, &format!("thread-fork:{source_work}:{review}"))).await
	}

	/// Bind only the acknowledged native fork after its exact prefix was read back.
	/// Install a canonical draft receipt for before-input branches without reverting either thread.
	pub async fn acknowledge_agent_fork(
		&self,
		attempt: AgentForkAttempt,
		target_thread: String,
		turns: Vec<String>,
	) -> Result<Option<AgentForkReceipt>, StoreError> {
		attempt.validate()?;
		if target_thread.is_empty()
			|| target_thread.len() > 512
			|| target_thread.chars().any(char::is_control)
			|| target_thread == attempt.source.thread
			|| attempt.expected_turns() != Some(turns.as_slice())
		{
			return Err(StoreError::InvalidInput("fork readback differs from reviewed prefix"));
		}
		self.run(move |c| {
			let tx=c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sqlite_error)?;
			let Some(saved)=receipt(&tx,&attempt.key())? else { return Ok(None); };
			if saved.attempt != attempt { return Ok(None); }
			if saved.state == "forked" { return Ok((saved.target_thread.as_deref()==Some(target_thread.as_str())).then_some(saved)); }
			if !matches!(saved.state.as_str(), "reserved" | "acknowledged") || saved.target_thread.as_ref().is_some_and(|thread| thread != &target_thread) { return Ok(None); }
			let target=read_work(&tx,&attempt.target_work)?;
			if target.codex_thread_id.is_some() || target.dispatch_state != crate::AgentDispatchState::Dispatching { return Ok(None); }
			let used:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_work_items WHERE codex_thread_id=?1)",[&target_thread],|r|r.get(0)).map_err(sqlite_error)?;
			if used { return Ok(None); }
			let now=unix_micros()?;
			tx.execute("UPDATE agent_work_items SET codex_thread_id=?2,dispatch_state='idle',updated_at_micros=max(updated_at_micros,?3) WHERE id=?1",params![attempt.target_work,target_thread,now]).map_err(sqlite_error)?;
			let edit_receipt_id=if attempt.boundary == AgentForkBoundary::BeforeInput {
				let mut edit=attempt.source.clone();
				edit.work=attempt.target_work.clone(); edit.thread=target_thread.clone();
				tx.execute("INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES(?1,?2,'prompt_edit_attempt',?3,?4,'resolved','reserved',?4)",params![edit.key(),edit.work,json!(edit).to_string(),now]).map_err(sqlite_error)?;
				let id=tx.last_insert_rowid();
				tx.execute("INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES(?1,?2,'prompt_edit_observation',?3,?4,'resolved','applied',?4)",params![format!("{}:observation",edit.key()),edit.work,json!({"generation":edit.generation,"turns":turns}).to_string(),now]).map_err(sqlite_error)?;
				Some(id)
			} else { None };
			tx.execute("INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES(?1,?2,'thread_fork_observation',?3,?4,'resolved','forked',?4)",params![format!("{}:observation",attempt.key()),attempt.source.work,json!({"thread":target_thread,"edit_receipt_id":edit_receipt_id}).to_string(),now]).map_err(sqlite_error)?;
			let saved=receipt(&tx,&attempt.key())?;
			tx.commit().map_err(sqlite_error)?;
			Ok(saved)
		}).await
	}
}

#[cfg(test)] mod tests;
