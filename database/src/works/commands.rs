use rusqlite::Connection;

use crate::{
	StoreError,
	error::sqlite_error,
	works::{self, collaboration},
};
use decodex_core::work::{
	Material, NativeContribution, Work, WorkAssessment, WorkCommand, WorkRequirements, WorkResult,
	WorkState,
};

pub(super) fn apply(
	c: &Connection,
	actor: &str,
	command: WorkCommand,
	user: bool,
) -> Result<(), StoreError> {
	match command {
		WorkCommand::PrepareSubagent { contribution: b } => prepare_subagent(c, actor, b),
		WorkCommand::BindSubagent {
			contribution_id,
			parent_turn_id,
			parent_item_id,
			child_thread_id,
		} => bind_subagent(
			c,
			actor,
			contribution_id,
			parent_turn_id,
			parent_item_id,
			child_thread_id,
		),
		WorkCommand::Create { work } => create(c, actor, work, user),
		WorkCommand::Revise { work_id, expected_requirements, requirements: r } =>
			revise(c, actor, work_id, expected_requirements, r, user),
		command @ WorkCommand::SetState { .. } => set_state(c, actor, command, user),
		WorkCommand::AddMaterial { material } => add_material(c, actor, material),
		WorkCommand::RecordResult { result } => record_result(c, actor, result),
		WorkCommand::ReuseResult { work_id, expected_requirements, result_id, assessment } =>
			reuse_result(c, actor, work_id, expected_requirements, result_id, assessment),
		WorkCommand::Complete { work_id, expected_requirements, assessment } =>
			complete(c, actor, work_id, expected_requirements, assessment),
		command => collaboration::apply(c, actor, command),
	}
}

fn prepare_subagent(c: &Connection, actor: &str, b: NativeContribution) -> Result<(), StoreError> {
	let w = works::owner(c, &b.work_id, actor, &b.requirements_id)?;

	works::active(&w)?;
	works::text(&b.id, 512)?;
	works::text(&b.instruction, 16_384)?;

	if b.child_thread_id.is_some() || b.parent_turn_id.is_some() || b.parent_item_id.is_some() {
		return Err(works::invalid("new native contribution must be unbound"));
	}

	c.execute("INSERT INTO native_work_contributions(id,work_id,requirements_id,instruction) VALUES(?1,?2,?3,?4)",rusqlite::params![b.id,b.work_id,b.requirements_id,b.instruction]).map_err(sqlite_error)?;

	Ok(())
}

fn bind_subagent(
	c: &Connection,
	actor: &str,
	contribution_id: String,
	parent_turn_id: String,
	parent_item_id: String,
	child_thread_id: String,
) -> Result<(), StoreError> {
	let (work, revision): (String, String) = c
		.query_row(
			"SELECT work_id,requirements_id FROM native_work_contributions WHERE id=?1",
			[&contribution_id],
			|r| Ok((r.get(0)?, r.get(1)?)),
		)
		.map_err(sqlite_error)?;

	works::owner(c, &work, actor, &revision)?;

	let n=c.execute("UPDATE native_work_contributions SET child_thread_id=?2,parent_turn_id=?3,parent_item_id=?4 WHERE id=?1 AND child_thread_id IS NULL",rusqlite::params![contribution_id,child_thread_id,parent_turn_id,parent_item_id]).map_err(sqlite_error)?;

	if n != 1 {
		return Err(works::conflict());
	}

	Ok(())
}

fn create(c: &Connection, actor: &str, work: Work, user: bool) -> Result<(), StoreError> {
	works::text(&work.id, 512)?;
	works::text(&work.title, 120)?;

	if work.owner_agent_id != actor
		|| work.state != WorkState::Open
		|| work.assessment.is_some()
		|| work.user_hold
		|| work.reason.is_some()
		|| work.resume_condition.is_some()
	{
		return Err(works::invalid("new Work must be open and owned by its creating Agent"));
	}

	works::authority(c, actor, &work.requirements.source, user)?;

	c.execute("INSERT INTO works(id,project_id,owner_agent_id,title,requirements_id,state) VALUES(?1,?2,?3,?4,?5,'open')",rusqlite::params![work.id,work.project_id,actor,work.title,work.requirements.id]).map_err(sqlite_error)?;

	works::requirements(c, &work.id, &work.requirements)?;

	Ok(())
}

fn revise(
	c: &Connection,
	actor: &str,
	work_id: String,
	expected_requirements: String,
	r: WorkRequirements,
	user: bool,
) -> Result<(), StoreError> {
	let w = works::owner(c, &work_id, actor, &expected_requirements)?;

	if w.state == WorkState::Cancelled {
		return Err(works::invalid("cancelled Work needs a new authorized assignment"));
	}

	works::authority(c, actor, &r.source, user)?;
	works::requirements(c, &work_id, &r)?;

	c.execute("UPDATE agent_inbox_events SET disposition='resolved',disposition_note='Requirements changed before admission; owner must issue a current instruction',disposed_at_micros=max(created_at_micros,?2) WHERE disposition IS NULL AND delivered_turn_id IS NULL AND id IN(SELECT i.event_id FROM collaboration_instructions i JOIN collaboration_requests q ON q.id=i.request_id WHERE q.work_id=?1)",rusqlite::params![work_id,crate::unix_micros()?]).map_err(sqlite_error)?;
	c.execute("UPDATE works SET requirements_id=?2,state=CASE WHEN user_hold=1 THEN 'blocked' ELSE 'open' END,assessment_json=NULL WHERE id=?1",rusqlite::params![work_id,r.id]).map_err(sqlite_error)?;

	Ok(())
}

fn set_state(
	c: &Connection,
	actor: &str,
	command: WorkCommand,
	user: bool,
) -> Result<(), StoreError> {
	let WorkCommand::SetState {
		work_id,
		expected_requirements,
		state,
		reason,
		resume_condition,
		user_hold,
	} = command
	else {
		return Err(works::invalid("expected state change"));
	};
	let w = works::owner(c, &work_id, actor, &expected_requirements)?;

	if state == WorkState::Completed
		|| w.state == WorkState::Completed
		|| w.state == WorkState::Cancelled
	{
		return Err(works::invalid(
			"completion needs an assessment; reopening needs authorized requirements",
		));
	}
	if (w.user_hold && !user) || (user_hold && !user) {
		return Err(works::invalid("a user hold requires explicit user release"));
	}
	if state == WorkState::Blocked {
		works::text(reason.as_deref().unwrap_or_default(), 4_096)?;
		works::text(resume_condition.as_deref().unwrap_or_default(), 4_096)?;
	}
	if state == WorkState::Cancelled {
		works::text(reason.as_deref().unwrap_or_default(), 4_096)?;
		collaboration::cancel_work(c, &work_id)?;
	}

	c.execute(
		"UPDATE works SET state=?2,reason=?3,resume_condition=?4,user_hold=?5 WHERE id=?1",
		rusqlite::params![work_id, works::state_name(state), reason, resume_condition, user_hold],
	)
	.map_err(sqlite_error)?;

	Ok(())
}

fn add_material(c: &Connection, actor: &str, material: Material) -> Result<(), StoreError> {
	let w = works::read_work(c, &material.work_id)?;

	works::owner(c, &w.id, actor, &w.requirements.id)?;
	works::active(&w)?;
	works::text(&material.id, 512)?;
	works::text(&material.title, 1_024)?;
	works::text(&material.reference, 4_096)?;

	c.execute(
		"INSERT INTO work_materials VALUES(?1,?2,?3)",
		rusqlite::params![material.id, material.work_id, works::encode(&material)?],
	)
	.map_err(sqlite_error)?;

	Ok(())
}

fn record_result(c: &Connection, actor: &str, result: WorkResult) -> Result<(), StoreError> {
	works::text(&result.id, 512)?;
	works::text(&result.summary, 16_384)?;

	if let Some(binding) = &result.native_contribution_id {
		let valid=c.query_row("SELECT EXISTS(SELECT 1 FROM native_work_contributions WHERE id=?1 AND work_id=?2 AND requirements_id=?3 AND child_thread_id IS NOT NULL)",rusqlite::params![binding,result.work_id,result.requirements_id],|r|r.get::<_,bool>(0)).map_err(sqlite_error)?;

		if !valid {
			return Err(works::invalid("native Result requires its exact bound contribution"));
		}
	}

	if result.agent_id != actor {
		return Err(works::invalid("a Result belongs to its actual contributing Agent"));
	}

	let w = works::read_work(c, &result.work_id)?;

	// Old and cancelled contributions are history. They cannot change current business
	// state.
	if let Some(instruction) = &result.instruction_id {
		collaboration::returned(c, actor, &w, &result, instruction)?;
	} else if w.owner_agent_id != actor {
		return Err(works::invalid("foreign Work requires an exact Collaboration instruction"));
	}

	c.execute(
		"INSERT INTO work_results VALUES(?1,?2,?3,?4,?5,?6)",
		rusqlite::params![
			result.id,
			result.work_id,
			result.requirements_id,
			result.instruction_id,
			actor,
			works::encode(&result)?
		],
	)
	.map_err(sqlite_error)?;

	Ok(())
}

fn reuse_result(
	c: &Connection,
	actor: &str,
	work_id: String,
	expected_requirements: String,
	result_id: String,
	assessment: String,
) -> Result<(), StoreError> {
	let w = works::owner(c, &work_id, actor, &expected_requirements)?;

	works::active(&w)?;
	works::text(&assessment, 16_384)?;

	let belongs = c
		.query_row(
			"SELECT EXISTS(SELECT 1 FROM work_results WHERE id=?1 AND work_id=?2)",
			rusqlite::params![result_id, work_id],
			|r| r.get::<_, bool>(0),
		)
		.map_err(sqlite_error)?;

	if !belongs {
		return Err(works::conflict());
	}

	c.execute(
		"INSERT INTO work_result_reuse VALUES(?1,?2,?3,?4)",
		rusqlite::params![work_id, expected_requirements, result_id, assessment],
	)
	.map_err(sqlite_error)?;

	Ok(())
}

fn complete(
	c: &Connection,
	actor: &str,
	work_id: String,
	expected_requirements: String,
	assessment: WorkAssessment,
) -> Result<(), StoreError> {
	let w = works::owner(c, &work_id, actor, &expected_requirements)?;

	works::active(&w)?;

	if assessment.requirements_id != expected_requirements
		|| assessment.result_ids.is_empty()
		|| assessment.result_ids.len() > 100
	{
		return Err(works::conflict());
	}

	works::text(&assessment.summary, 16_384)?;

	for id in &assessment.result_ids {
		let valid=c.query_row("SELECT EXISTS(SELECT 1 FROM work_results r WHERE r.id=?1 AND r.work_id=?2 AND (r.requirements_id=?3 OR EXISTS(SELECT 1 FROM work_result_reuse v WHERE v.result_id=r.id AND v.work_id=r.work_id AND v.requirements_id=?3)))",rusqlite::params![id,work_id,expected_requirements],|r|r.get::<_,bool>(0)).map_err(sqlite_error)?;

		if !valid {
			return Err(works::invalid(
				"completion needs related Results checked against current requirements",
			));
		}
	}

	let waiting=c.query_row("SELECT EXISTS(SELECT 1 FROM collaboration_requests WHERE work_id=?1 AND required=1 AND state<>'closed')",[&work_id],|r|r.get::<_,bool>(0)).map_err(sqlite_error)?;

	if waiting {
		return Err(works::invalid("assess or cancel required contributions before completion"));
	}

	c.execute("UPDATE works SET state='completed',reason=NULL,resume_condition=NULL,assessment_json=?2 WHERE id=?1",rusqlite::params![work_id,works::encode(&assessment)?]).map_err(sqlite_error)?;

	Ok(())
}
