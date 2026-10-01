//! Shared native config arbitration. Keep existing hook event identities readable.
use rusqlite::{Connection, OptionalExtension as _};
use serde::{Deserialize, Serialize};

use crate::{StoreError, agent_process, error};

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct AgentConfigOwner {
	pub work: String,
	pub thread: String,
	pub generation: String,
	pub account: String,
}

pub(crate) fn text(value: &str) -> bool {
	!value.trim().is_empty() && value.len() <= 4_096 && !value.chars().any(char::is_control)
}
pub(crate) fn digest(value: &str) -> bool {
	value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}
pub(crate) fn owned(c: &Connection, owner: &AgentConfigOwner) -> Result<bool, StoreError> {
	if !agent_process::owns_work(c, &owner.work, Some(&owner.generation))? {
		return Ok(false);
	}

	c.query_row("SELECT EXISTS(SELECT 1 FROM agent_work_items w JOIN process_generations g ON g.generation_id=?3 WHERE w.id=?1 AND w.codex_thread_id=?2 AND w.status<>'resolved' AND g.account_id=?4 AND g.state='ready')",rusqlite::params![owner.work,owner.thread,owner.generation,owner.account],|r|r.get(0)).map_err(|e|error::sqlite_error(e).into())
}

/// Called inside the caller's immediate transaction, before any setting reservation.
/// A config file has one outstanding writer, regardless of task, account or setting kind.
pub(crate) fn available(c: &Connection, scope: &str, review: &str) -> Result<bool, StoreError> {
	let blocked: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM agent_inbox_events a
         LEFT JOIN agent_inbox_events r ON r.source_event_id=CASE a.event_kind WHEN 'hook_setting_attempt' THEN 'hook-result:' ELSE 'app-result:' END||a.id
         LEFT JOIN agent_inbox_events o ON o.source_event_id=CASE a.event_kind WHEN 'hook_setting_attempt' THEN 'hook-observation:' ELSE 'app-observation:' END||a.id
         WHERE a.event_kind IN ('hook_setting_attempt','app_setting_attempt')
         AND json_extract(a.payload,'$.scope')=?1
         AND (COALESCE(o.disposition_note,r.disposition_note,'reserved') IN ('reserved','unknown')
              OR json_extract(a.payload,'$.review_token')=?2))",
        rusqlite::params![scope,review], |r|r.get(0)).map_err(error::sqlite_error)?;

	Ok(!blocked)
}

pub(crate) fn dead(c: &Connection, generation: &str) -> Result<bool, StoreError> {
	c.query_row("SELECT EXISTS(SELECT 1 FROM process_generations g JOIN process_generation_death_evidence e ON e.evidence_id=g.death_evidence_id AND e.generation_id=g.generation_id WHERE g.generation_id=?1 AND g.state='dead')",[generation],|r|r.get(0)).map_err(|e|error::sqlite_error(e).into())
}

/// Latest shared receipt identity lets consumers disclose a writer from another setting surface.
pub(crate) fn latest_identity(
	c: &Connection,
	scope: &str,
) -> Result<Option<(i64, String)>, StoreError> {
	c.query_row("SELECT id,event_kind FROM agent_inbox_events WHERE event_kind IN ('hook_setting_attempt','app_setting_attempt') AND json_extract(payload,'$.scope')=?1 ORDER BY id DESC LIMIT 1",[scope],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(|e|error::sqlite_error(e).into())
}
