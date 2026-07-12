use std::{
	path::PathBuf,
	time::{SystemTime, UNIX_EPOCH},
};

use rusqlite::OptionalExtension;

use crate::{
	lane_authority::{
		LaneAggregate, LaneCommand, LaneId, LanePhase, LaneTransitionRejection, transition,
	},
	state::sqlite_store::{Result, SqliteStateStore, eyre, params},
};

impl SqliteStateStore {
	pub(in crate::state) fn transition_lane(
		&mut self,
		id: &LaneId,
		expected_epoch: u64,
		binding_fingerprint: &str,
		command: LaneCommand,
	) -> Result<LaneAggregate> {
		let updated_at_unix =
			i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())?;
		let transaction = self.connection.transaction()?;
		let persisted = transaction
			.query_row(
				"SELECT binding_fingerprint, epoch, phase, intake_authority_id, claim_run_id, branch_name, worktree_path \
				 FROM lanes WHERE project_key = ?1 AND tracker_issue_id = ?2",
				params![id.project_key(), id.tracker_issue_id()],
				|row| {
					let phase_value: String = row.get(2)?;
					let phase = LanePhase::from_str(&phase_value).ok_or_else(|| {
						rusqlite::Error::InvalidColumnType(
							2,
							String::from("phase"),
							rusqlite::types::Type::Text,
						)
					})?;
					let epoch_value: i64 = row.get(1)?;
					let epoch = u64::try_from(epoch_value)
						.map_err(|_| rusqlite::Error::IntegralValueOutOfRange(1, epoch_value))?;
					Ok(LaneAggregate::from_persisted_parts(
						id.clone(),
						row.get(0)?,
						epoch,
						phase,
						row.get(3)?,
						row.get(4)?,
						row.get(5)?,
						row.get::<_, Option<String>>(6)?.map(PathBuf::from),
					))
				},
			)
			.optional()?;
		let current = persisted
			.clone()
			.unwrap_or_else(|| LaneAggregate::new(id.clone(), binding_fingerprint));
		let next = transition(&current, expected_epoch, binding_fingerprint, command)
			.map_err(|rejection| eyre::eyre!("lane_transition_rejected:{rejection:?}"))?;
		if next == current {
			transaction.commit()?;
			return Ok(next);
		}

		if next.phase().holds_active_authority() {
			let conflicting_project = transaction
				.query_row(
					"SELECT project_key FROM lanes WHERE tracker_issue_id = ?1 \
					 AND project_key <> ?2 \
					 AND phase IN ('claimed', 'running', 'waiting_review') LIMIT 1",
					params![id.tracker_issue_id(), id.project_key()],
					|row| row.get::<_, String>(0),
				)
				.optional()?;
			if conflicting_project.is_some() {
				return Err(eyre::eyre!(
					"lane_transition_rejected:{:?}",
					LaneTransitionRejection::TrackerIssueAlreadyActive
				));
			}
		}

		let changed = if persisted.is_some() {
			transaction.execute(
				"UPDATE lanes SET epoch = ?3, phase = ?4, intake_authority_id = ?5, claim_run_id = ?6, branch_name = ?7, \
				 worktree_path = ?8, updated_at_unix = ?9 \
				 WHERE project_key = ?1 AND tracker_issue_id = ?2 AND epoch = ?10 \
				 AND binding_fingerprint = ?11",
				params![
					id.project_key(),
					id.tracker_issue_id(),
					i64::try_from(next.epoch())?,
					next.phase().as_str(),
					next.intake_authority_id(),
					next.claim_run_id(),
					next.branch_name(),
					next.worktree_path().map(|path| path.to_string_lossy().into_owned()),
					updated_at_unix,
					i64::try_from(current.epoch())?,
					current.binding_fingerprint(),
				],
			)?
		} else {
			transaction.execute(
				"INSERT INTO lanes (project_key, tracker_issue_id, binding_fingerprint, epoch, phase, \
				 intake_authority_id, claim_run_id, branch_name, worktree_path, updated_at_unix) \
				 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
				params![
					id.project_key(),
					id.tracker_issue_id(),
					next.binding_fingerprint(),
					i64::try_from(next.epoch())?,
					next.phase().as_str(),
					next.intake_authority_id(),
					next.claim_run_id(),
					next.branch_name(),
					next.worktree_path().map(|path| path.to_string_lossy().into_owned()),
					updated_at_unix,
				],
			)?
		};
		if changed != 1 {
			eyre::bail!("lane_compare_and_swap_failed");
		}
		transaction.commit()?;
		Ok(next)
	}
}
