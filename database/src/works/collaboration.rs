use super::{active, agent, conflict, encode, invalid, owner, read_work, request, text};
use crate::{StoreError, error::sqlite_error};
use decodex_core::work::{
	CollaborationInstruction, CollaborationRequest, CollaborationState, Work, WorkCommand,
	WorkResult,
};
use rusqlite::{Connection, params};

fn notice(
	c: &Connection,
	agent: &str,
	source: String,
	kind: &str,
	payload: String,
) -> Result<i64, StoreError> {
	c.execute("INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros) VALUES(?1,?2,?3,?4,?5)",params![source,agent,kind,payload,crate::unix_micros()?]).map_err(sqlite_error)?;
	Ok(c.last_insert_rowid())
}
fn retire(c: &Connection, event: Option<i64>, note: &str) -> Result<(), StoreError> {
	c.execute("UPDATE agent_inbox_events SET disposition='resolved',disposition_note=?2,disposed_at_micros=max(created_at_micros,?3) WHERE id=?1 AND disposition IS NULL",params![event,note,crate::unix_micros()?]).map_err(sqlite_error)?;
	Ok(())
}
fn instruction(
	c: &Connection,
	q: &CollaborationRequest,
	i: &CollaborationInstruction,
	w: &Work,
) -> Result<(), StoreError> {
	text(&i.id, 512)?;
	text(&i.text, 16384)?;
	if i.requirements_id != w.requirements.id
		|| i.event_id.is_some()
		|| i.delivered_turn_id.is_some()
		|| i.material_ids.len() > 100
	{
		return Err(conflict());
	}
	let mut materials = Vec::new();
	for id in &i.material_ids {
		let json: String = c
			.query_row(
				"SELECT record_json FROM work_materials WHERE work_id=?1 AND id=?2",
				params![w.id, id],
				|r| r.get(0),
			)
			.map_err(sqlite_error)?;
		materials.push(
			serde_json::from_str::<serde_json::Value>(&json)
				.map_err(|_| invalid("invalid Material"))?,
		);
	}
	let payload = serde_json::json!({"source":"agent","requestId":q.id,"instructionId":i.id,"workId":w.id,"requirements":w.requirements,"text":i.text,"materials":materials,"requestingAgentId":w.owner_agent_id,"authority":"application contribution request; not user input; share only selected Materials"});
	let event = notice(
		c,
		&q.recipient_agent_id,
		serde_json::json!(["collaboration", q.id, i.id]).to_string(),
		"collaboration_request",
		encode(&payload)?,
	)?;
	c.execute(
		"INSERT INTO collaboration_instructions VALUES(?1,?2,?3,?4,?5,?6)",
		params![q.id, i.id, i.requirements_id, i.text, encode(&i.material_ids)?, event],
	)
	.map_err(sqlite_error)?;
	Ok(())
}
fn recipient(
	c: &Connection,
	actor: &str,
	id: &str,
	instruction: &str,
) -> Result<CollaborationRequest, StoreError> {
	let q = request(c, id)?;
	if q.recipient_agent_id != actor || q.instruction.id != instruction {
		return Err(conflict());
	}
	let delivered = q.instruction.delivered_turn_id.as_deref().is_some_and(|t| !t.is_empty());
	if !delivered {
		return Err(invalid("accept only the exact delivered instruction"));
	}
	Ok(q)
}
pub(super) fn returned(
	c: &Connection,
	actor: &str,
	w: &Work,
	r: &WorkResult,
	instruction: &str,
) -> Result<(), StoreError> {
	let (id,revision,recipient_id):(String,String,String)=c.query_row("SELECT q.id,i.requirements_id,q.recipient_agent_id FROM collaboration_instructions i JOIN collaboration_requests q ON q.id=i.request_id WHERE i.id=?1",[instruction],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(sqlite_error)?;
	if recipient_id != actor || revision != r.requirements_id {
		return Err(conflict());
	}
	let q = request(c, &id)?;
	if q.work_id != w.id {
		return Err(conflict());
	}
	// Late returns remain evidence. They never satisfy a newer instruction or revive cancelled
	// Work.
	if q.instruction.id == instruction
		&& q.state != CollaborationState::Closed
		&& matches!(
			w.state,
			decodex_core::work::WorkState::Open | decodex_core::work::WorkState::Blocked
		)
		&& w.requirements.id == revision
	{
		recipient(c, actor, &id, instruction)?;
		c.execute("UPDATE collaboration_requests SET state='returned' WHERE id=?1", [&id])
			.map_err(sqlite_error)?;
		retire(
			c,
			q.instruction.event_id,
			"Contribution returned; requesting owner must assess it",
		)?;
		notice(
			c,
			&w.owner_agent_id,
			serde_json::json!(["collaboration_result", r.id]).to_string(),
			"collaboration_result",
			encode(
				&serde_json::json!({"source":"agent","workId":w.id,"requestId":id,"instructionId":instruction,"result":r}),
			)?,
		)?;
	}
	Ok(())
}
pub(super) fn cancel_work(c: &Connection, work: &str) -> Result<(), StoreError> {
	let requests = super::ids(
		c,
		"SELECT id FROM collaboration_requests WHERE work_id=?1 AND state<>'closed'",
		[Some(work)],
	)?;
	for id in requests {
		let q = request(c, &id)?;
		retire(
			c,
			q.instruction.event_id,
			"Request cancelled; unrelated recipient execution is unchanged",
		)?;
	}
	c.execute("UPDATE collaboration_requests SET state='closed',outcome='cancelled',note='Work cancelled; no new related admission' WHERE work_id=?1 AND state<>'closed'",[work]).map_err(sqlite_error)?;
	Ok(())
}
pub(super) fn apply(c: &Connection, actor: &str, command: WorkCommand) -> Result<(), StoreError> {
	match command {
		WorkCommand::Request { request: q } => {
			text(&q.id, 512)?;
			agent(c, &q.recipient_agent_id)?;
			let w = owner(c, &q.work_id, actor, &q.instruction.requirements_id)?;
			active(&w)?;
			if q.recipient_agent_id == actor
				|| q.state != CollaborationState::Pending
				|| q.outcome.is_some()
				|| q.note.is_some()
			{
				return Err(invalid(
					"new Collaboration Request must be pending and target another Agent",
				));
			}
			c.execute("INSERT INTO collaboration_requests(id,work_id,recipient_agent_id,instruction_id,state,required) VALUES(?1,?2,?3,?4,'pending',?5)",params![q.id,q.work_id,q.recipient_agent_id,q.instruction.id,q.required]).map_err(sqlite_error)?;
			instruction(c, &q, &q.instruction, &w)?;
		},
		WorkCommand::Respond { request_id, instruction_id, accept, note } => {
			let q = recipient(c, actor, &request_id, &instruction_id)?;
			let w = read_work(c, &q.work_id)?;
			active(&w)?;
			if q.state != CollaborationState::Pending
				|| q.instruction.requirements_id != w.requirements.id
			{
				return Err(conflict());
			}
			text(&note, 4096)?;
			c.execute(
				"UPDATE collaboration_requests SET state=?2,outcome=?3,note=?4 WHERE id=?1",
				params![
					request_id,
					if accept { "accepted" } else { "closed" },
					if accept { None } else { Some("declined") },
					note
				],
			)
			.map_err(sqlite_error)?;
			retire(
				c,
				q.instruction.event_id,
				"Recipient explicitly responded; delivery is separate from acceptance",
			)?;
			if !accept {
				notice(
					c,
					&w.owner_agent_id,
					serde_json::json!(["collaboration_declined", request_id, instruction_id])
						.to_string(),
					"collaboration_result",
					encode(
						&serde_json::json!({"source":"agent","requestId":request_id,"outcome":"declined","note":note}),
					)?,
				)?;
			}
		},
		WorkCommand::Repair { request_id, expected_instruction, instruction: i } => {
			let q = request(c, &request_id)?;
			let w = owner(c, &q.work_id, actor, &i.requirements_id)?;
			active(&w)?;
			if q.instruction.id != expected_instruction || q.outcome.as_deref() == Some("cancelled")
			{
				return Err(conflict());
			}
			retire(
				c,
				q.instruction.event_id,
				"Superseded by an explicit repair instruction; in-flight effects need reconciliation",
			)?;
			instruction(c, &q, &i, &w)?;
			c.execute("UPDATE collaboration_requests SET instruction_id=?2,state='pending',outcome=NULL,note=NULL WHERE id=?1",params![request_id,i.id]).map_err(sqlite_error)?;
		},
		WorkCommand::CloseRequest { request_id, instruction_id, outcome, note } => {
			let q = request(c, &request_id)?;
			let w = read_work(c, &q.work_id)?;
			owner(c, &w.id, actor, &w.requirements.id)?;
			if q.instruction.id != instruction_id || q.state == CollaborationState::Closed {
				return Err(conflict());
			}
			text(&note, 4096)?;
			if !["used", "not_used", "cancelled", "failed"].contains(&outcome.as_str())
				|| (outcome == "used"
					&& (q.state != CollaborationState::Returned
						|| q.instruction.requirements_id != w.requirements.id))
			{
				return Err(invalid("use only a returned contribution for current requirements"));
			}
			c.execute(
				"UPDATE collaboration_requests SET state='closed',outcome=?2,note=?3 WHERE id=?1",
				params![request_id, outcome, note],
			)
			.map_err(sqlite_error)?;
			retire(
				c,
				q.instruction.event_id,
				"Contribution assessed or cancelled by its Work owner",
			)?;
		},
		_ => return Err(invalid("unsupported Collaboration command")),
	}
	Ok(())
}
