use std::path::PathBuf;

use rusqlite::OptionalExtension;

use crate::{
	lane_authority::{
		LaneAggregate, LaneCommand, LanePhase, RepairHandoffAuthority, SupersessionEdge, transition,
	},
	prelude::{Result, eyre},
	state::sqlite_store::{SqliteStateStore, mutations::params},
};

impl SqliteStateStore {
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
