//! Durable Work ownership. Transactions fence requirements, requests and assessments.
mod collaboration;
mod commands;

use std::collections::HashSet;

use rusqlite::{Connection, OptionalExtension as _, Params, TransactionBehavior};
use serde::{Serialize, de::DeserializeOwned};

use crate::{DatabaseError, SqliteStore, StoreError, error};
use decodex_core::work::{
	CollaborationInstruction, CollaborationRequest, CollaborationState, NativeAssignmentProof,
	NativeWorkDelivery, Work, WorkCommand, WorkPortfolio, WorkRequirements, WorkResult, WorkState,
};

const PAGE_SIZE: usize = 100;

impl SqliteStore {
	/// Read an exact committed business receipt before repeating external evidence checks.
	pub async fn work_command_applied(
		&self,
		actor: String,
		id: String,
		command: WorkCommand,
	) -> Result<bool, StoreError> {
		let json = encode(&command)?;

		self.run(move |c| {
			let saved = c
				.query_row(
					"SELECT agent_id,command_json FROM work_commands WHERE id=?1",
					[id],
					|r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
				)
				.optional()
				.map_err(crate::error::sqlite_error)?;

			match saved {
				None => Ok(false),
				Some((saved_actor, saved_json)) if saved_actor == actor && saved_json == json =>
					Ok(true),
				Some(_) => Err(conflict()),
			}
		})
		.await
	}

	/// Read current business state without touching native execution or admitting input.
	pub async fn work_portfolio(
		&self,
		work_id: Option<String>,
	) -> Result<WorkPortfolio, StoreError> {
		self.scoped_work_portfolio(work_id, None).await
	}

	async fn scoped_work_portfolio(
		&self,
		work_id: Option<String>,
		actor: Option<String>,
	) -> Result<WorkPortfolio, StoreError> {
		self.run(move |c| {
			let tx = c.transaction().map_err(crate::error::sqlite_error)?;
			let mut works = ids(&tx,
				"SELECT w.id FROM works w WHERE (?1 IS NULL OR w.id=?1) AND (?2 IS NULL OR w.owner_agent_id=?2 OR EXISTS(SELECT 1 FROM collaboration_requests q WHERE q.work_id=w.id AND q.recipient_agent_id=?2)) ORDER BY CASE w.state WHEN 'open' THEN 0 WHEN 'blocked' THEN 1 ELSE 2 END,w.id LIMIT 101",
				rusqlite::params![work_id, actor])?.into_iter().map(|id| read_work(&tx,&id)).collect::<Result<Vec<_>,_>>()?;
			let work_has_more = works.len() > PAGE_SIZE;

			works.truncate(PAGE_SIZE);

			let selected = serde_json::to_string(&works.iter().map(|w| &w.id).collect::<Vec<_>>()).map_err(|_| invalid("invalid Work selection"))?;
			let collaborations = ids(&tx,
				"SELECT id FROM collaboration_requests WHERE work_id IN(SELECT value FROM json_each(?1)) AND (?2 IS NULL OR recipient_agent_id=?2 OR work_id IN(SELECT id FROM works WHERE owner_agent_id=?2)) ORDER BY CASE state WHEN 'closed' THEN 1 ELSE 0 END,id LIMIT 101",
				rusqlite::params![selected, actor])?.into_iter().map(|id|request(&tx,&id)).collect::<Result<Vec<_>,_>>()?;
			let results = json_rows(&tx,
				"SELECT record_json FROM work_results WHERE work_id IN(SELECT value FROM json_each(?1)) AND (?2 IS NULL OR agent_id=?2 OR work_id IN(SELECT id FROM works WHERE owner_agent_id=?2)) ORDER BY rowid DESC LIMIT 101",
				rusqlite::params![selected, actor])?;
			let materials = json_rows(&tx,
				"SELECT m.record_json FROM work_materials m WHERE m.work_id IN(SELECT value FROM json_each(?1)) AND (?2 IS NULL OR m.work_id IN(SELECT id FROM works WHERE owner_agent_id=?2) OR EXISTS(SELECT 1 FROM collaboration_requests q JOIN collaboration_instructions i ON i.id=q.instruction_id JOIN json_each(i.material_ids_json) g ON g.value=m.id WHERE q.work_id=m.work_id AND q.recipient_agent_id=?2)) ORDER BY m.rowid DESC LIMIT 101",
				rusqlite::params![selected, actor])?;
			let native_contributions=tx.prepare("SELECT id,work_id,requirements_id,instruction,child_thread_id,parent_turn_id,parent_item_id FROM native_work_contributions WHERE work_id IN(SELECT value FROM json_each(?1)) AND (?2 IS NULL OR work_id IN(SELECT id FROM works WHERE owner_agent_id=?2)) ORDER BY rowid DESC LIMIT 101").map_err(crate::error::sqlite_error)?.query_map(rusqlite::params![selected, actor],|r|Ok(decodex_core::work::NativeContribution{id:r.get(0)?,work_id:r.get(1)?,requirements_id:r.get(2)?,instruction:r.get(3)?,child_thread_id:r.get(4)?,parent_turn_id:r.get(5)?,parent_item_id:r.get(6)?})).map_err(crate::error::sqlite_error)?.collect::<Result<_,_>>().map_err(crate::error::sqlite_error)?;
			let mut result=WorkPortfolio{awaiting_assessment:assessment_pending(&tx,Some(&selected))?,summary_only:false,native_contributions,works,collaborations,results,materials,has_more:work_has_more};

			result.has_more |= [result.collaborations.len(),result.results.len(),result.materials.len(),result.native_contributions.len()].iter().any(|n|*n>PAGE_SIZE);

			result.collaborations.truncate(PAGE_SIZE);result.results.truncate(PAGE_SIZE);result.materials.truncate(PAGE_SIZE);result.native_contributions.truncate(PAGE_SIZE);

			tx.commit().map_err(crate::error::sqlite_error)?;Ok(result)
		}).await
	}

	/// Read bounded Work headers without loading Materials, Results or private conversation text.
	pub async fn work_overview(&self) -> Result<WorkPortfolio, StoreError> {
		self.run(move |c| {
			let mut works =
				ids(c, "SELECT id FROM works WHERE ?1 IS NULL ORDER BY CASE state WHEN 'open' THEN 0 WHEN 'blocked' THEN 1 ELSE 2 END,id LIMIT 101", [None::<&str>])?
					.into_iter()
					.map(|id| read_work(c, &id))
					.collect::<Result<Vec<_>, _>>()?;
			let has_more = works.len() > PAGE_SIZE;

			works.truncate(PAGE_SIZE);

			for work in &mut works {
				work.requirements.goal.clear();
				work.requirements.criteria.clear();
				work.requirements.source.clear();

				work.assessment = None;
				work.reason = None;
				work.resume_condition = None;
			}

			Ok(WorkPortfolio {
				awaiting_assessment: assessment_pending(c, None)?,
				works,
				has_more,
				summary_only: true,
				..Default::default()
			})
		})
		.await
	}

	/// Apply one authorized, idempotent mutation. Native Result sources must be verified by the
	/// runtime first.
	pub async fn apply_work_command(
		&self,
		actor: String,
		command_id: String,
		command: WorkCommand,
		user_authority: bool,
	) -> Result<(), StoreError> {
		text(&command_id, 512)?;

		let json = encode(&command)?;

		self.run(move |c| {
			let tx = c
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(crate::error::sqlite_error)?;

			agent(&tx, &actor)?;

			if let Some((saved_actor, saved)) = tx
				.query_row(
					"SELECT agent_id,command_json FROM work_commands WHERE id=?1",
					[&command_id],
					|r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
				)
				.optional()
				.map_err(crate::error::sqlite_error)?
			{
				return if saved_actor == actor && saved == json {
					Ok(())
				} else {
					Err(conflict())
				};
			}

			commands::apply(&tx, &actor, command, user_authority)?;

			tx.execute(
				"INSERT INTO work_commands VALUES(?1,?2,?3)",
				rusqlite::params![command_id, actor, json],
			)
			.map_err(crate::error::sqlite_error)?;
			tx.commit().map_err(crate::error::sqlite_error)?;

			Ok(())
		})
		.await
	}
}

impl SqliteStore {
	/// Read only owned outcomes and current scoped requests. This grants no conversation history.
	pub async fn agent_work_portfolio(
		&self,
		agent_id: String,
		work_id: Option<String>,
	) -> Result<WorkPortfolio, StoreError> {
		let mut portfolio = self.scoped_work_portfolio(work_id, Some(agent_id.clone())).await?;
		let owned: HashSet<_> = portfolio
			.works
			.iter()
			.filter(|w| w.owner_agent_id == agent_id)
			.map(|w| w.id.clone())
			.collect();

		portfolio.awaiting_assessment.retain(|id| owned.contains(id));

		for work in &mut portfolio.works {
			if !owned.contains(&work.id) {
				work.assessment = None;
				work.reason = None;
				work.resume_condition = None;
			}
		}

		Ok(portfolio)
	}

	/// Existing persistent collaborators. Native Subagents and legacy managed executors are
	/// excluded.
	pub async fn persistent_agents(&self) -> Result<Vec<(String, String)>, StoreError> {
		self.run(|c| {
			c.prepare(
				"SELECT a.id,w.title FROM agents a JOIN agent_work_items w ON w.id=a.id ORDER BY a.id",
			)
			.map_err(crate::error::sqlite_error)?
			.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
			.map_err(crate::error::sqlite_error)?
			.collect::<Result<_, _>>()
			.map_err(|e| error::sqlite_error(e).into())
		})
		.await
	}
}

impl SqliteStore {
	pub async fn native_contribution(
		&self,
		id: String,
	) -> Result<decodex_core::work::NativeContribution, StoreError> {
		self.run(move|c|c.query_row("SELECT id,work_id,requirements_id,instruction,child_thread_id,parent_turn_id,parent_item_id FROM native_work_contributions WHERE id=?1",[id],|r|Ok(decodex_core::work::NativeContribution{id:r.get(0)?,work_id:r.get(1)?,requirements_id:r.get(2)?,instruction:r.get(3)?,child_thread_id:r.get(4)?,parent_turn_id:r.get(5)?,parent_item_id:r.get(6)?})).map_err(|e|error::sqlite_error(e).into())).await
	}
}

impl SqliteStore {
	pub async fn agents(&self) -> Result<Vec<decodex_core::work::Agent>, StoreError> {
		self.run(|c|c.prepare("SELECT a.id,w.title,w.codex_thread_id,a.assistant FROM agents a JOIN agent_work_items w ON w.id=a.id ORDER BY a.id").map_err(crate::error::sqlite_error)?.query_map([],|r|Ok(decodex_core::work::Agent{id:r.get(0)?,name:r.get(1)?,native_thread_id:r.get(2)?,assistant:r.get(3)?})).map_err(crate::error::sqlite_error)?.collect::<Result<_,_>>().map_err(|e|error::sqlite_error(e).into())).await
	}

	pub async fn set_assistant(&self, agent: String, enabled: bool) -> Result<(), StoreError> {
		self.run(move |c| {
			let tx = c.transaction().map_err(crate::error::sqlite_error)?;

			if enabled {
				tx.execute("UPDATE agents SET assistant=0 WHERE assistant=1", [])
					.map_err(crate::error::sqlite_error)?;
			}

			let n = tx
				.execute(
					"UPDATE agents SET assistant=?2 WHERE id=?1",
					rusqlite::params![agent, enabled],
				)
				.map_err(crate::error::sqlite_error)?;

			if n != 1 {
				return Err(conflict());
			}

			tx.commit().map_err(crate::error::sqlite_error)?;

			Ok(())
		})
		.await
	}
}

impl SqliteStore {
	/// Read one exact retained Result from its Work, including older bounded-view entries.
	pub async fn work_result(&self, work: String, id: String) -> Result<WorkResult, StoreError> {
		self.run(move |c| {
			let record = c
				.query_row(
					"SELECT record_json FROM work_results WHERE work_id=?1 AND id=?2",
					rusqlite::params![work, id],
					|r| r.get::<_, String>(0),
				)
				.map_err(crate::error::sqlite_error)?;

			decode(record)
		})
		.await
	}
}

impl SqliteStore {
	/// Retain an observed native call only after its exact prompt matches saved attribution intent.
	pub async fn record_native_work_dispatch(
		&self,
		id: String,
		thread: String,
		turn: String,
		call: String,
	) -> Result<(), StoreError> {
		for value in [&id, &thread, &turn, &call] {
			text(value, 512)?;
		}

		self.run(move|c|{let n=c.execute("INSERT INTO native_work_dispatches(contribution_id,thread_id,turn_id,call_id) VALUES(?1,?2,?3,?4) ON CONFLICT(contribution_id) DO UPDATE SET thread_id=excluded.thread_id WHERE thread_id=excluded.thread_id AND turn_id=excluded.turn_id AND call_id=excluded.call_id",rusqlite::params![id,thread,turn,call]).map_err(crate::error::sqlite_error)?;if n!=1{return Err(conflict());}Ok(())}).await
	}

	/// Preserve the exact native receiving turn and message without replaying input.
	pub async fn record_native_work_delivery(
		&self,
		id: String,
		delivery: NativeWorkDelivery,
	) -> Result<(), StoreError> {
		for value in [
			&id,
			&delivery.parent_thread_id,
			&delivery.parent_turn_id,
			&delivery.child_thread_id,
			&delivery.child_turn_id,
			&delivery.message_id,
		] {
			text(value, 512)?;
		}

		self.run(move |c| {
			let n = c.execute(
				"INSERT INTO native_work_deliveries VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(contribution_id) DO UPDATE SET message_id=excluded.message_id WHERE parent_thread_id=excluded.parent_thread_id AND parent_turn_id=excluded.parent_turn_id AND child_thread_id=excluded.child_thread_id AND child_turn_id=excluded.child_turn_id AND message_id=excluded.message_id AND assignment_proof=excluded.assignment_proof",
				rusqlite::params![id, delivery.parent_thread_id, delivery.parent_turn_id, delivery.child_thread_id, delivery.child_turn_id, delivery.message_id, delivery.assignment_proof.as_str()],
			).map_err(crate::error::sqlite_error)?;

			if n != 1 { return Err(conflict()); }

			Ok(())
		}).await
	}

	/// Read retained inbound assignment identity. Missing evidence remains unknown.
	pub async fn native_work_delivery(
		&self,
		id: String,
	) -> Result<Option<NativeWorkDelivery>, StoreError> {
		self.run(move |c| {
			c.query_row(
				"SELECT parent_thread_id,parent_turn_id,child_thread_id,child_turn_id,message_id,assignment_proof FROM native_work_deliveries WHERE contribution_id=?1",
				[id], |r| Ok(NativeWorkDelivery { parent_thread_id:r.get(0)?, parent_turn_id:r.get(1)?, child_thread_id:r.get(2)?, child_turn_id:r.get(3)?, message_id:r.get(4)?, assignment_proof: if r.get::<_,String>(5)? == "native_task_name" {NativeAssignmentProof::NativeTaskName} else {NativeAssignmentProof::Plaintext} }),
			).optional().map_err(|e| error::sqlite_error(e).into())
		}).await
	}

	/// Read the exact retained dispatch receipt. Its absence never authorizes another spawn.
	pub async fn native_work_dispatch(
		&self,
		id: String,
	) -> Result<Option<(String, String, String)>, StoreError> {
		self.run(move |c| {
			c.query_row(
				"SELECT thread_id,turn_id,call_id FROM native_work_dispatches WHERE contribution_id=?1",
				[id],
				|r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
			)
			.optional()
			.map_err(|e| error::sqlite_error(e).into())
		})
		.await
	}
}

fn invalid(message: &'static str) -> StoreError {
	StoreError::InvalidInput(message)
}

fn conflict() -> StoreError {
	DatabaseError::Conflict.into()
}

fn text(value: &str, max: usize) -> Result<(), StoreError> {
	if value.trim().is_empty() || value.len() > max {
		return Err(invalid("invalid Work text or identity"));
	}

	Ok(())
}

fn encode<T>(value: &T) -> Result<String, StoreError>
where
	T: Serialize,
{
	let json = serde_json::to_string(value).map_err(|_| invalid("invalid Work record"))?;

	if json.len() > 65_536 {
		return Err(invalid("Work record exceeds 64 KiB"));
	}

	Ok(json)
}

fn decode<T>(value: String) -> Result<T, StoreError>
where
	T: DeserializeOwned,
{
	serde_json::from_str(&value).map_err(|_| invalid("invalid retained Work record"))
}

fn state_name(state: WorkState) -> &'static str {
	match state {
		WorkState::Open => "open",
		WorkState::Blocked => "blocked",
		WorkState::Completed => "completed",
		WorkState::Cancelled => "cancelled",
	}
}

fn read_work(c: &Connection, id: &str) -> Result<Work, StoreError> {
	let row=c.query_row("SELECT w.project_id,w.owner_agent_id,w.title,w.requirements_id,w.state,w.reason,w.resume_condition,w.user_hold,w.assessment_json,r.goal,r.criteria,r.source FROM works w JOIN work_requirements r ON r.work_id=w.id AND r.id=w.requirements_id WHERE w.id=?1",[id],|r| Ok((r.get::<_,Option<String>>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?,r.get::<_,Option<String>>(5)?,r.get::<_,Option<String>>(6)?,r.get::<_,bool>(7)?,r.get::<_,Option<String>>(8)?,r.get::<_,String>(9)?,r.get::<_,String>(10)?,r.get::<_,String>(11)?))).map_err(crate::error::sqlite_error)?;

	Ok(Work {
		id: id.into(),
		project_id: row.0,
		owner_agent_id: row.1,
		title: row.2,
		requirements: WorkRequirements { id: row.3, goal: row.9, criteria: row.10, source: row.11 },
		state: match row.4.as_str() {
			"open" => WorkState::Open,
			"blocked" => WorkState::Blocked,
			"completed" => WorkState::Completed,
			"cancelled" => WorkState::Cancelled,
			_ => return Err(conflict()),
		},
		reason: row.5,
		resume_condition: row.6,
		user_hold: row.7,
		assessment: row.8.map(decode).transpose()?,
	})
}

fn owner(c: &Connection, id: &str, agent: &str, revision: &str) -> Result<Work, StoreError> {
	let work = read_work(c, id)?;

	if work.owner_agent_id != agent || work.requirements.id != revision {
		return Err(conflict());
	}

	Ok(work)
}

fn active(work: &Work) -> Result<(), StoreError> {
	if work.state != WorkState::Open {
		return Err(invalid("Work is not open"));
	}

	Ok(())
}

fn agent(c: &Connection, id: &str) -> Result<(), StoreError> {
	if !c
		.query_row("SELECT EXISTS(SELECT 1 FROM agents WHERE id=?1)", [id], |r| r.get::<_, bool>(0))
		.map_err(crate::error::sqlite_error)?
	{
		return Err(invalid("expected an existing persistent Agent"));
	}

	Ok(())
}

fn requirements(c: &Connection, work: &str, r: &WorkRequirements) -> Result<(), StoreError> {
	text(&r.id, 512)?;
	text(&r.goal, 16_384)?;
	text(&r.criteria, 16_384)?;
	text(&r.source, 1_024)?;

	c.execute(
		"INSERT INTO work_requirements VALUES(?1,?2,?3,?4,?5)",
		rusqlite::params![work, r.id, r.goal, r.criteria, r.source],
	)
	.map_err(crate::error::sqlite_error)?;

	Ok(())
}

fn authority(c: &Connection, agent: &str, source: &str, user: bool) -> Result<(), StoreError> {
	if user {
		return Ok(());
	}

	let event =
		source.strip_prefix("event:").and_then(|s| s.parse::<i64>().ok()).ok_or_else(|| {
			invalid("cite an exact event:<id> user decision from the delivered inbox")
		})?;
	let valid=c.query_row("SELECT EXISTS(SELECT 1 FROM agent_inbox_events e JOIN agent_work_items a ON a.id=?1 WHERE e.id=?2 AND e.work_item_id=?1 AND e.event_kind IN('user_message','async_question_answer') AND e.delivered_turn_id IS NOT NULL AND e.delivered_turn_id<>'' AND (json_extract(e.payload,'$.source')='user' OR e.event_kind='async_question_answer'))",rusqlite::params![agent,event],|r|r.get::<_,bool>(0)).map_err(crate::error::sqlite_error)?;

	if !valid {
		return Err(invalid("requirements need a current delivered user decision"));
	}

	Ok(())
}

fn request(c: &Connection, id: &str) -> Result<CollaborationRequest, StoreError> {
	let row=c.query_row("SELECT q.work_id,q.recipient_agent_id,q.instruction_id,q.state,q.required,q.outcome,q.note,i.requirements_id,i.text,i.material_ids_json,i.event_id,e.delivered_turn_id FROM collaboration_requests q JOIN collaboration_instructions i ON i.request_id=q.id AND i.id=q.instruction_id JOIN agent_inbox_events e ON e.id=i.event_id WHERE q.id=?1",[id],|r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,bool>(4)?,r.get::<_,Option<String>>(5)?,r.get::<_,Option<String>>(6)?,r.get::<_,String>(7)?,r.get::<_,String>(8)?,r.get::<_,String>(9)?,r.get::<_,i64>(10)?,r.get::<_,Option<String>>(11)?))).map_err(crate::error::sqlite_error)?;

	Ok(CollaborationRequest {
		id: id.into(),
		work_id: row.0,
		recipient_agent_id: row.1,
		instruction: CollaborationInstruction {
			id: row.2,
			requirements_id: row.7,
			text: row.8,
			material_ids: decode(row.9)?,
			event_id: Some(row.10),
			delivered_turn_id: row.11,
		},
		state: match row.3.as_str() {
			"pending" => CollaborationState::Pending,
			"accepted" => CollaborationState::Accepted,
			"returned" => CollaborationState::Returned,
			_ => CollaborationState::Closed,
		},
		required: row.4,
		outcome: row.5,
		note: row.6,
	})
}

fn ids(c: &Connection, sql: &str, values: impl Params) -> Result<Vec<String>, StoreError> {
	c.prepare(sql)
		.map_err(crate::error::sqlite_error)?
		.query_map(values, |r| r.get(0))
		.map_err(crate::error::sqlite_error)?
		.collect::<Result<_, _>>()
		.map_err(|e| error::sqlite_error(e).into())
}

fn json_rows<T>(c: &Connection, sql: &str, values: impl Params) -> Result<Vec<T>, StoreError>
where
	T: DeserializeOwned,
{
	ids(c, sql, values)?.into_iter().map(decode).collect()
}

fn assessment_pending(c: &Connection, work: Option<&str>) -> Result<Vec<String>, StoreError> {
	ids(
		c,
		"SELECT w.id FROM works w WHERE (?1 IS NULL OR w.id IN(SELECT value FROM json_each(?1))) AND w.state='open' AND EXISTS(SELECT 1 FROM work_results r WHERE r.work_id=w.id AND (r.requirements_id=w.requirements_id OR EXISTS(SELECT 1 FROM work_result_reuse u WHERE u.work_id=w.id AND u.requirements_id=w.requirements_id AND u.result_id=r.id))) ORDER BY w.id LIMIT 100",
		[work],
	)
}

#[cfg(test)] mod tests;
