//! One durable native fork intent and its independent local work owner.

use rusqlite::{Connection, OptionalExtension as _, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
	AgentDispatchState, AgentPromptEditAttempt, AgentWorkStatus, SqliteStore, StoreError, agent,
	agent_prompt_edit, error,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentForkBoundary {
	BeforeInput,
	AfterTurn,
	AttachedAfterTurn,
	/// Native in-memory fork owned by the open side chat.
	TemporarySideChat,
}

impl AgentForkBoundary {
	/// A side question shares its source's tab strip.
	pub fn is_side_chat(self) -> bool {
		matches!(self, Self::AttachedAfterTurn | Self::TemporarySideChat)
	}
}

/// Thread identity is sufficient to open a live attachment. Historical forks retain their exact
/// review.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct AgentForkSource {
	pub work: String,
	pub thread: String,
	pub generation: Option<String>,
	pub review_token: String,
	#[serde(flatten)]
	pub review: Option<ForkHistory>,
}
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ForkHistory {
	pub attempt_id: String,
	pub before_turn_id: String,
	item_id: String,
	pub turn_ids: Vec<String>,
	pub content: Vec<Value>,
}
impl From<AgentPromptEditAttempt> for AgentForkSource {
	fn from(a: AgentPromptEditAttempt) -> Self {
		Self {
			work: a.work,
			thread: a.thread,
			generation: a.generation,
			review_token: a.review_token,
			review: Some(ForkHistory {
				attempt_id: a.attempt_id,
				before_turn_id: a.before_turn_id,
				item_id: a.item_id,
				turn_ids: a.turn_ids,
				content: a.content,
			}),
		}
	}
}
impl AgentForkSource {
	fn prompt(&self) -> Option<AgentPromptEditAttempt> {
		let r = self.review.as_ref()?;
		Some(AgentPromptEditAttempt {
			work: self.work.clone(),
			thread: self.thread.clone(),
			generation: self.generation.clone(),
			review_token: self.review_token.clone(),
			attempt_id: r.attempt_id.clone(),
			before_turn_id: r.before_turn_id.clone(),
			item_id: r.item_id.clone(),
			turn_ids: r.turn_ids.clone(),
			content: r.content.clone(),
		})
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct AgentForkAttempt {
	pub source: AgentForkSource,
	pub target_work: String,
	pub boundary: AgentForkBoundary,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub model_provider: Option<String>,
}
impl AgentForkAttempt {
	fn receipt_owner(&self) -> &str {
		if self.boundary == AgentForkBoundary::TemporarySideChat {
			&self.target_work
		} else {
			&self.source.work
		}
	}

	fn key(&self) -> String {
		format!("thread-fork:{}:{}", self.source.work, self.source.review_token)
	}

	fn validate(&self) -> Result<(), StoreError> {
		if let Some(review) = self.source.prompt() {
			review.validate()?;
		} else if !self.boundary.is_side_chat()
			|| self.source.review_token.len() != 64
			|| !self.source.review_token.bytes().all(|c| c.is_ascii_hexdigit())
			|| [&self.source.work, &self.source.thread]
				.into_iter()
				.chain(self.source.generation.iter())
				.any(|v| v.is_empty() || v.len() > 512 || v.chars().any(char::is_control))
		{
			return Err(StoreError::InvalidInput("invalid side chat source"));
		}
		if self.model_provider.as_ref().is_some_and(|provider| {
			self.boundary != AgentForkBoundary::AfterTurn
				|| provider.is_empty()
				|| provider.len() > 512
				|| provider.chars().any(char::is_control)
		}) {
			return Err(StoreError::InvalidInput("invalid recovery provider"));
		}

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
		if self.boundary.is_side_chat() {
			return Some(&[]);
		}
		let review = self.source.review.as_ref()?;
		let boundary = review.turn_ids.iter().position(|t| t == &review.before_turn_id)?;
		let end = boundary + usize::from(self.boundary != AgentForkBoundary::BeforeInput);
		Some(&review.turn_ids[..end])
	}
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
			let tx=c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(error::sqlite_error)?;
			let Some(saved)=receipt(&tx,&attempt.key())? else { return Ok(None); };

			if saved.attempt != attempt { return Ok(None); }
			if matches!(saved.state.as_str(), "acknowledged" | "forked") { return Ok((saved.target_thread.as_deref()==Some(target_thread.as_str())).then_some(saved)); }
			if saved.state != "reserved" { return Ok(None); }

			let now=crate::unix_micros()?;

			tx.execute("INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES(?1,?2,'thread_fork_identity',?3,?4,'resolved','acknowledged',?4)",rusqlite::params![format!("{}:acknowledgement",attempt.key()),attempt.receipt_owner(),serde_json::json!({"thread":target_thread}).to_string(),now]).map_err(error::sqlite_error)?;

			let saved=receipt(&tx,&attempt.key())?;

			tx.commit().map_err(error::sqlite_error)?;

			Ok(saved)
		}).await
	}

	/// Record a proved pre-write or invalid-request refusal. Never use this for timeouts.
	pub async fn reject_agent_fork(&self, attempt: AgentForkAttempt) -> Result<bool, StoreError> {
		attempt.validate()?;

		self.run(move |c| {
			let tx=c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(error::sqlite_error)?;
			let Some(saved)=receipt(&tx,&attempt.key())? else { return Ok(false); };

			if saved.attempt != attempt || saved.state != "reserved" { return Ok(false); }

			let now=crate::unix_micros()?;
			let changed=tx.execute("UPDATE agent_work_items SET status='resolved',dispatch_state='idle',updated_at_micros=max(updated_at_micros,?2) WHERE id=?1 AND codex_thread_id IS NULL AND dispatch_state='dispatching'",rusqlite::params![attempt.target_work,now]).map_err(error::sqlite_error)?;

			if changed!=1 { return Ok(false); }

			tx.execute("INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES(?1,?2,'thread_fork_observation','{}',?3,'resolved','rejected',?3)",rusqlite::params![format!("{}:observation",attempt.key()),attempt.receipt_owner(),now]).map_err(error::sqlite_error)?;
			tx.commit().map_err(error::sqlite_error)?;

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
			let tx = c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(error::sqlite_error)?;

			if receipt(&tx, &attempt.key())?.is_some() { return Ok(None); }

			let a=&attempt.source;
			let source = agent::read_work(&tx, &a.work)?;

			// A live side question reads its source. A history edit retains its writer guard.
			let may_fork = if a.review.is_none() {
				crate::agent_process::account_thread(&tx, &a.work, &a.thread, a.generation.as_deref())?
			} else {
				agent_prompt_edit::owns_thread(&tx, &a.work, &a.thread, a.generation.as_deref())?
			};
			if !may_fork
				|| source.codex_thread_id.as_deref() != Some(a.thread.as_str())
				|| (a.review.is_some() && (source.dispatch_state != AgentDispatchState::Idle || source.active_turn_id.is_some() || source.status == AgentWorkStatus::Resolved))
				|| agent_prompt_edit::pending(&tx, &a.work)?
			{ return Ok(None); }

			let attached = attempt.boundary.is_side_chat();
			let mut title = format!("{} ({})", source.title, if attached { "Side chat" } else { "Fork" });

			while title.len() > 1_024 { title.pop(); }

			let source_is_agent = agent::is_agent(&tx, &source.id)?;
            // Ordinary conversations have no execution parent. The fork receipt keeps
            // the native history and side-chat relationship independently.
            let parent = if source.parent_goal_id.is_none() && !source_is_agent {
                None
            } else if attached {
                Some(source.id.as_str())
            } else {
                source.parent_goal_id.as_deref().or(Some(source.id.as_str()))
            };
			let now=crate::unix_micros()?;

			tx.execute("INSERT INTO agent_work_items(id,parent_goal_id,kind,title,instructions,status,dispatch_state,created_at_micros,updated_at_micros) VALUES(?1,?2,?3,?4,?5,'open','dispatching',?6,?6)", rusqlite::params![attempt.target_work,parent,source.kind.as_str(),title,source.instructions,now]).map_err(error::sqlite_error)?;
			// Preserve the source tool version; a fork is not a tool-upgrade mechanism.
			tx.execute("INSERT INTO agent_tool_versions(work_id,version) SELECT ?1,version FROM agent_tool_versions WHERE work_id=?2", rusqlite::params![attempt.target_work,a.work]).map_err(error::sqlite_error)?;

			if source_is_agent {
				agent::insert_agent(&tx, &attempt.target_work, true, false)?;
			}

            tx.execute("INSERT INTO project_agents(agent_id,project_id) SELECT ?1,project_id FROM project_agents WHERE agent_id=?2", rusqlite::params![attempt.target_work,a.work]).map_err(error::sqlite_error)?;
			tx.execute("INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES(?1,?2,?5,?3,?4,'resolved','reserved',?4)", rusqlite::params![attempt.key(),attempt.receipt_owner(),serde_json::json!(attempt).to_string(),now,if attempt.boundary==AgentForkBoundary::TemporarySideChat {"temporary_side_chat"} else {"thread_fork_attempt"}]).map_err(error::sqlite_error)?;

			let saved=receipt(&tx,&attempt.key())?;

			tx.commit().map_err(error::sqlite_error)?;

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

	/// Read a bounded retained attachment index, including uncertain attempts, without native
	/// effects.
	pub async fn agent_attached_forks(
		&self,
		source_work: String,
	) -> Result<(Vec<AgentForkReceipt>, bool), StoreError> {
		self.run(move |c| {
			agent::read_work(c, &source_work)?;

			let keys = {
				let mut query=c.prepare("SELECT source_event_id FROM (
                    SELECT id,source_event_id FROM agent_inbox_events WHERE work_item_id=?1 AND event_kind='thread_fork_attempt' AND json_extract(payload,'$.boundary')='attached_after_turn'
                    UNION ALL SELECT e.id,e.source_event_id FROM agent_work_items w CROSS JOIN agent_inbox_events e ON e.work_item_id=w.id AND e.event_kind='temporary_side_chat' WHERE json_extract(e.payload,'$.source.work')=?1
                ) ORDER BY id DESC LIMIT 101").map_err(error::sqlite_error)?;

				query.query_map([source_work],|r|r.get::<_,String>(0)).map_err(error::sqlite_error)?.collect::<Result<Vec<_>,_>>().map_err(error::sqlite_error)?
			};
			let truncated=keys.len()>100;
			let forks=keys.into_iter().take(100).map(|key|receipt(c,&key)?.ok_or(StoreError::InvalidInput("missing side chat receipt"))).collect::<Result<Vec<_>,_>>()?;

			Ok((forks,truncated))
		}).await
	}

	/// Bind the acknowledged native fork after its expected visible history was read back.
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
			let tx=c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(error::sqlite_error)?;
			let Some(saved)=receipt(&tx,&attempt.key())? else { return Ok(None); };

			if saved.attempt != attempt { return Ok(None); }
			if saved.state == "forked" { return Ok((saved.target_thread.as_deref()==Some(target_thread.as_str())).then_some(saved)); }
			if !matches!(saved.state.as_str(), "reserved" | "acknowledged") || saved.target_thread.as_ref().is_some_and(|thread| thread != &target_thread) { return Ok(None); }

			let target=agent::read_work(&tx,&attempt.target_work)?;

			if target.codex_thread_id.is_some() || target.dispatch_state != AgentDispatchState::Dispatching { return Ok(None); }

			let used:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_work_items WHERE codex_thread_id=?1)",[&target_thread],|r|r.get(0)).map_err(error::sqlite_error)?;

			if used { return Ok(None); }

			let now=crate::unix_micros()?;

			tx.execute("UPDATE agent_work_items SET codex_thread_id=?2,dispatch_state='idle',updated_at_micros=max(updated_at_micros,?3) WHERE id=?1",rusqlite::params![attempt.target_work,target_thread,now]).map_err(error::sqlite_error)?;

			let edit_receipt_id=if attempt.boundary == AgentForkBoundary::BeforeInput {
				let mut edit=attempt.source.prompt().ok_or(StoreError::InvalidInput("missing fork history"))?;

				edit.work=attempt.target_work.clone(); edit.thread=target_thread.clone();
				tx.execute("INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES(?1,?2,'prompt_edit_attempt',?3,?4,'resolved','reserved',?4)",rusqlite::params![edit.key(),edit.work,serde_json::json!(edit).to_string(),now]).map_err(error::sqlite_error)?;

				let id=tx.last_insert_rowid();

				tx.execute("INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES(?1,?2,'prompt_edit_observation',?3,?4,'resolved','applied',?4)",rusqlite::params![format!("{}:observation",edit.key()),edit.work,serde_json::json!({"generation":edit.generation,"turns":turns}).to_string(),now]).map_err(error::sqlite_error)?;

				Some(id)
			} else { None };

			tx.execute("INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES(?1,?2,'thread_fork_observation',?3,?4,'resolved','forked',?4)",rusqlite::params![format!("{}:observation",attempt.key()),attempt.receipt_owner(),serde_json::json!({"thread":target_thread,"edit_receipt_id":edit_receipt_id}).to_string(),now]).map_err(error::sqlite_error)?;

			let saved=receipt(&tx,&attempt.key())?;

			tx.commit().map_err(error::sqlite_error)?;

			Ok(saved)
		}).await
	}
}

/// Use the reservation so navigation never publishes a pending child as an independent chat.
pub(crate) fn side_chat_sources(
	c: &Connection,
) -> Result<std::collections::BTreeMap<String, String>, StoreError> {
	let mut q=c.prepare("SELECT child.id, parent.id FROM agent_work_items parent CROSS JOIN agent_inbox_events f ON f.work_item_id=parent.id AND f.event_kind='thread_fork_attempt' JOIN agent_work_items child ON child.id=json_extract(f.payload,'$.target_work') WHERE json_extract(f.payload,'$.boundary')='attached_after_turn'
        UNION ALL SELECT w.id,json_extract(e.payload,'$.source.work') FROM agent_work_items w CROSS JOIN agent_inbox_events e ON e.work_item_id=w.id AND e.event_kind='temporary_side_chat'").map_err(error::sqlite_error)?;
	Ok(q.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
		.map_err(error::sqlite_error)?
		.collect::<Result<_, _>>()
		.map_err(error::sqlite_error)?)
}
fn receipt(c: &Connection, key: &str) -> Result<Option<AgentForkReceipt>, StoreError> {
	let saved: Option<(i64,String,String,Option<String>)> = c.query_row(
		"SELECT a.id,a.payload,coalesce(o.disposition_note,k.disposition_note,'reserved'),coalesce(o.payload,k.payload) FROM agent_inbox_events a LEFT JOIN agent_inbox_events k ON k.source_event_id=a.source_event_id||':acknowledgement' AND k.event_kind='thread_fork_identity' LEFT JOIN agent_inbox_events o ON o.source_event_id=a.source_event_id||':observation' AND o.event_kind='thread_fork_observation' WHERE a.source_event_id=?1 AND a.event_kind IN ('thread_fork_attempt','temporary_side_chat')",
		[key], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)),
	).optional().map_err(error::sqlite_error)?;

	saved
		.map(|(id, payload, state, observed)| {
			let attempt: AgentForkAttempt = serde_json::from_str(&payload)
				.map_err(|_| StoreError::InvalidInput("invalid saved fork"))?;

			attempt.validate()?;

			let observed: Value = observed
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

#[cfg(test)] mod tests;
