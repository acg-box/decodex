use std::path::PathBuf;

use rusqlite::OptionalExtension;

use crate::{
	lane_authority::{
		LaneAggregate, LaneCommand, LanePhase, RepairHandoffAuthority, SupersededCloseoutCommand,
		SupersededCloseoutOperation, SupersessionEdge, transition, transition_superseded_closeout,
	},
	prelude::{Result, eyre},
	state::sqlite_store::{SqliteStateStore, mutations::params},
};

impl SqliteStateStore {
	pub(in crate::state) fn replace_repair_handoff(
		&mut self,
		current: &RepairHandoffAuthority,
		replacement: &RepairHandoffAuthority,
	) -> Result<()> {
		if current.handoff_id() == replacement.handoff_id()
			|| current.predecessor_lane_id() != replacement.predecessor_lane_id()
			|| current.predecessor_epoch() != replacement.predecessor_epoch()
		{
			eyre::bail!("Repair handoff replacement changes frozen predecessor authority.");
		}
		let transaction = self.connection.transaction()?;
		let current_epoch = transaction.query_row(
			"SELECT epoch FROM lanes WHERE project_key = ?1 AND tracker_issue_id = ?2",
			params![
				current.predecessor_lane_id().project_key(),
				current.predecessor_lane_id().tracker_issue_id()
			],
			|row| row.get::<_, i64>(0),
		)?;
		if u64::try_from(current_epoch)? != current.predecessor_epoch() {
			eyre::bail!("Repair handoff replacement predecessor epoch is stale.");
		}
		let replaced = transaction.execute(
			"UPDATE repair_handoffs SET state = 'replaced'
			 WHERE handoff_id = ?1 AND state = 'active' AND predecessor_epoch = ?2",
			params![current.handoff_id(), i64::try_from(current.predecessor_epoch())?],
		)?;
		if replaced != 1 {
			eyre::bail!("Repair handoff replacement lost the active-handoff CAS.");
		}
		transaction.execute(
			"INSERT INTO repair_handoffs (
				handoff_id, predecessor_project_key, predecessor_issue_id, predecessor_epoch,
				successor_project_key, successor_issue_id, state, payload_json, created_at_unix
			) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'active', ?7, unixepoch())",
			params![
				replacement.handoff_id(),
				replacement.predecessor_lane_id().project_key(),
				replacement.predecessor_lane_id().tracker_issue_id(),
				i64::try_from(replacement.predecessor_epoch())?,
				replacement.successor_lane_id().project_key(),
				replacement.successor_lane_id().tracker_issue_id(),
				serde_json::to_string(replacement)?,
			],
		)?;
		transaction.commit()?;
		Ok(())
	}

	pub(in crate::state) fn advance_superseded_closeout(
		&mut self,
		current: &SupersededCloseoutOperation,
		next: &SupersededCloseoutOperation,
	) -> Result<()> {
		if current == next {
			return Ok(());
		}
		let transaction = self.connection.transaction()?;
		if next.stage() == crate::lane_authority::SupersededCloseoutStage::Terminal {
			let lane = current.edge().predecessor_lane_id();
			let lane_changed = transaction.execute(
				"UPDATE lanes SET phase = 'terminal', epoch = epoch + 1, updated_at_unix = unixepoch()
				 WHERE project_key = ?1 AND tracker_issue_id = ?2
				 AND phase = 'terminal_cleanup_pending'",
				params![lane.project_key(), lane.tracker_issue_id()],
			)?;
			if lane_changed != 1 {
				eyre::bail!("Superseded closeout terminal Lane CAS failed.");
			}
		}
		let changed = transaction.execute(
			"UPDATE superseded_closeout_operations
			 SET stage = ?2, stage_epoch = ?3, payload_json = ?4, updated_at_unix = unixepoch()
			 WHERE operation_id = ?1 AND stage = ?5 AND stage_epoch = ?6",
			params![
				current.operation_id(),
				next.stage().as_str(),
				i64::try_from(next.stage_epoch())?,
				serde_json::to_string(next)?,
				current.stage().as_str(),
				i64::try_from(current.stage_epoch())?,
			],
		)?;
		if changed != 1 {
			eyre::bail!("Superseded closeout operation stage CAS failed.");
		}
		transaction.commit()?;
		Ok(())
	}

	pub(in crate::state) fn insert_superseded_closeout_operation(
		&self,
		operation: &SupersededCloseoutOperation,
	) -> Result<()> {
		let payload = serde_json::to_string(operation)?;
		let edge = operation.edge();
		let inserted = self.connection.execute(
			"INSERT OR IGNORE INTO superseded_closeout_operations (
				operation_id, edge_id, predecessor_project_key, predecessor_issue_id,
				stage, stage_epoch, payload_json, updated_at_unix
			) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, unixepoch())",
			params![
				operation.operation_id(),
				edge.edge_id(),
				edge.predecessor_lane_id().project_key(),
				edge.predecessor_lane_id().tracker_issue_id(),
				operation.stage().as_str(),
				i64::try_from(operation.stage_epoch())?,
				payload,
			],
		)?;
		if inserted == 1 {
			return Ok(());
		}
		let existing = self.connection.query_row(
			"SELECT payload_json FROM superseded_closeout_operations WHERE operation_id = ?1",
			params![operation.operation_id()],
			|row| row.get::<_, String>(0),
		)?;
		if existing != serde_json::to_string(operation)? {
			eyre::bail!("Superseded closeout operation authority-key collision.");
		}
		Ok(())
	}

	pub(in crate::state) fn insert_repair_handoff(
		&mut self,
		handoff: &RepairHandoffAuthority,
	) -> Result<()> {
		let transaction = self.connection.transaction()?;
		for lane_id in [handoff.predecessor_lane_id(), handoff.successor_lane_id()] {
			let exists = transaction.query_row(
				"SELECT EXISTS(SELECT 1 FROM lanes WHERE project_key = ?1 AND tracker_issue_id = ?2)",
				params![lane_id.project_key(), lane_id.tracker_issue_id()],
				|row| row.get::<_, bool>(0),
			)?;
			if !exists {
				eyre::bail!("Repair handoff references an unknown canonical lane.");
			}
		}
		let predecessor_epoch = transaction.query_row(
			"SELECT epoch FROM lanes WHERE project_key = ?1 AND tracker_issue_id = ?2",
			params![
				handoff.predecessor_lane_id().project_key(),
				handoff.predecessor_lane_id().tracker_issue_id()
			],
			|row| row.get::<_, i64>(0),
		)?;
		if u64::try_from(predecessor_epoch)? != handoff.predecessor_epoch() {
			eyre::bail!("Repair handoff predecessor epoch is stale.");
		}
		let inserted = transaction.execute(
			"INSERT OR IGNORE INTO repair_handoffs (
				handoff_id, predecessor_project_key, predecessor_issue_id, predecessor_epoch,
				successor_project_key, successor_issue_id, state, payload_json, created_at_unix
			) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'active', ?7, unixepoch())",
			params![
				handoff.handoff_id(),
				handoff.predecessor_lane_id().project_key(),
				handoff.predecessor_lane_id().tracker_issue_id(),
				i64::try_from(handoff.predecessor_epoch())?,
				handoff.successor_lane_id().project_key(),
				handoff.successor_lane_id().tracker_issue_id(),
				serde_json::to_string(handoff)?,
			],
		)?;
		if inserted != 1 {
			let existing = transaction
				.query_row(
					"SELECT payload_json FROM repair_handoffs WHERE handoff_id = ?1",
					params![handoff.handoff_id()],
					|row| row.get::<_, String>(0),
				)
				.optional()?;
			if existing.as_deref() != Some(serde_json::to_string(handoff)?.as_str()) {
				eyre::bail!("Repair handoff conflicts with existing active authority.");
			}
		}
		transaction.commit()?;
		Ok(())
	}

	pub(in crate::state) fn commit_supersession(
		&mut self,
		handoff: &RepairHandoffAuthority,
		edge: &SupersessionEdge,
		operation: &SupersededCloseoutOperation,
		binding_fingerprint: &str,
	) -> Result<LaneAggregate> {
		let transaction = self.connection.transaction()?;
		let id = handoff.predecessor_lane_id();
		let current = transaction.query_row(
			"SELECT binding_fingerprint, epoch, phase, intake_authority_id, claim_run_id,
			 branch_name, worktree_path FROM lanes
			 WHERE project_key = ?1 AND tracker_issue_id = ?2",
			params![id.project_key(), id.tracker_issue_id()],
			|row| {
				let phase_text: String = row.get(2)?;
				let phase = LanePhase::from_str(&phase_text).ok_or_else(|| {
					rusqlite::Error::InvalidColumnType(
						2,
						String::from("phase"),
						rusqlite::types::Type::Text,
					)
				})?;
				let epoch_value: i64 = row.get(1)?;
				Ok(LaneAggregate::from_persisted_parts(
					id.clone(),
					row.get(0)?,
					u64::try_from(epoch_value)
						.map_err(|_| rusqlite::Error::IntegralValueOutOfRange(1, epoch_value))?,
					phase,
					row.get(3)?,
					row.get(4)?,
					row.get(5)?,
					row.get::<_, Option<String>>(6)?.map(PathBuf::from),
				))
			},
		)?;
		if current.epoch() != handoff.predecessor_epoch() {
			eyre::bail!("Supersession predecessor epoch drifted before terminal commit.");
		}
		let next = transition(
			&current,
			current.epoch(),
			binding_fingerprint,
			LaneCommand::BeginSupersededCleanup,
		)
		.map_err(|rejection| eyre::eyre!("supersession_lane_transition_rejected:{rejection:?}"))?;
		let handoff_changed = transaction.execute(
			"UPDATE repair_handoffs SET state = 'accepted'
			 WHERE handoff_id = ?1 AND state = 'active' AND predecessor_epoch = ?2",
			params![handoff.handoff_id(), i64::try_from(current.epoch())?],
		)?;
		if handoff_changed != 1 {
			eyre::bail!("Supersession handoff CAS rejected stale authority.");
		}
		transaction.execute(
			"INSERT INTO supersession_edges (
				edge_id, handoff_id, predecessor_project_key, predecessor_issue_id,
				predecessor_epoch, payload_json, created_at_unix
			) VALUES (?1, ?2, ?3, ?4, ?5, ?6, unixepoch())",
			params![
				edge.edge_id(),
				edge.handoff_id(),
				edge.predecessor_lane_id().project_key(),
				edge.predecessor_lane_id().tracker_issue_id(),
				i64::try_from(edge.predecessor_epoch())?,
				serde_json::to_string(edge)?,
			],
		)?;
		let lane_changed = transaction.execute(
			"UPDATE lanes SET epoch = ?3, phase = ?4, claim_run_id = NULL,
			 updated_at_unix = unixepoch()
			 WHERE project_key = ?1 AND tracker_issue_id = ?2 AND epoch = ?5
			 AND binding_fingerprint = ?6",
			params![
				id.project_key(),
				id.tracker_issue_id(),
				i64::try_from(next.epoch())?,
				next.phase().as_str(),
				i64::try_from(current.epoch())?,
				binding_fingerprint,
			],
		)?;
		if lane_changed != 1 {
			eyre::bail!("Supersession lane CAS failed.");
		}
		let next_operation = transition_superseded_closeout(
			operation,
			operation.stage_epoch(),
			SupersededCloseoutCommand::CommitTerminalAuthority,
		)
		.map_err(|rejection| eyre::eyre!("superseded_closeout_stage_rejected:{rejection:?}"))?;
		for effect in operation.planned_effects(binding_fingerprint)? {
			transaction.execute(
				"INSERT INTO lane_effects (
					effect_id, operation_id, ordinal, project_key, tracker_issue_id,
					journal_epoch, kind, payload_json, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, unixepoch())",
				params![
					effect.effect_id(),
					effect.operation_id(),
					i64::from(effect.ordinal()),
					effect.lane_id().project_key(),
					effect.lane_id().tracker_issue_id(),
					i64::try_from(effect.journal_epoch())?,
					effect.kind().registry_name(),
					serde_json::to_string(&effect)?,
				],
			)?;
		}
		let operation_changed = transaction.execute(
			"UPDATE superseded_closeout_operations
			 SET stage = ?2, stage_epoch = ?3, payload_json = ?4, updated_at_unix = unixepoch()
			 WHERE operation_id = ?1 AND stage = 'acceptance_attested' AND stage_epoch = ?5",
			params![
				operation.operation_id(),
				next_operation.stage().as_str(),
				i64::try_from(next_operation.stage_epoch())?,
				serde_json::to_string(&next_operation)?,
				i64::try_from(operation.stage_epoch())?,
			],
		)?;
		if operation_changed != 1 {
			eyre::bail!("Superseded closeout operation stage CAS failed.");
		}
		if let Some(run_id) = current.claim_run_id() {
			let released = transaction.execute(
				"DELETE FROM leases WHERE issue_id = ?1 AND project_id = ?2 AND run_id = ?3",
				params![id.tracker_issue_id(), id.project_key(), run_id],
			)?;
			if released != 1 {
				eyre::bail!("Supersession exact conflict lease release failed.");
			}
		}
		transaction.commit()?;
		Ok(next)
	}
}
