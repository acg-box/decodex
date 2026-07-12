#[cfg(test)] use crate::lane_authority::LaneTransitionRejection;
use crate::{
	lane_authority::{LaneAggregate, LaneCommand, LaneId, transition},
	prelude::{Result, eyre},
	state::StateStore,
};

impl StateStore {
	pub(crate) fn lane(&self, id: &LaneId) -> Result<Option<LaneAggregate>> {
		Ok(self
			.inner
			.lock()
			.map_err(|_| eyre::eyre!("State lock poisoned."))?
			.lanes
			.get(id)
			.cloned())
	}

	#[cfg(test)]
	pub(crate) fn transition_lane(
		&self,
		id: LaneId,
		expected_epoch: u64,
		binding_fingerprint: &str,
		command: LaneCommand,
	) -> Result<LaneAggregate> {
		if let Some(sqlite) = &self.sqlite {
			let next = sqlite
				.lock()
				.map_err(|_| eyre::eyre!("SQLite state lock poisoned."))?
				.transition_lane(&id, expected_epoch, binding_fingerprint, command)?;
			self.inner
				.lock()
				.map_err(|_| eyre::eyre!("State lock poisoned."))?
				.lanes
				.insert(id, next.clone());
			return Ok(next);
		}

		let mut state = self.inner.lock().map_err(|_| eyre::eyre!("State lock poisoned."))?;
		let persisted = state.lanes.get(&id).cloned();
		let current = persisted
			.clone()
			.unwrap_or_else(|| LaneAggregate::new(id.clone(), binding_fingerprint));
		let next = transition(&current, expected_epoch, binding_fingerprint, command)
			.map_err(|rejection| eyre::eyre!("lane_transition_rejected:{rejection:?}"))?;

		if next.phase().holds_active_authority()
			&& state.lanes.values().any(|lane| {
				lane.id() != &id
					&& lane.id().tracker_issue_id() == id.tracker_issue_id()
					&& lane.phase().holds_active_authority()
			}) {
			return Err(eyre::eyre!(
				"lane_transition_rejected:{:?}",
				LaneTransitionRejection::TrackerIssueAlreadyActive
			));
		}

		if next == current {
			return Ok(next);
		}
		state.lanes.insert(id, next.clone());
		Ok(next)
	}

	pub(crate) fn apply_lane_command(
		&self,
		id: LaneId,
		binding_fingerprint: &str,
		command: LaneCommand,
	) -> Result<LaneAggregate> {
		if let Some(sqlite) = &self.sqlite {
			let mut sqlite =
				sqlite.lock().map_err(|_| eyre::eyre!("SQLite state lock poisoned."))?;
			for _ in 0..3 {
				let expected_epoch = sqlite.lane(&id)?.map_or(0, |lane| lane.epoch());
				match sqlite.transition_lane(
					&id,
					expected_epoch,
					binding_fingerprint,
					command.clone(),
				) {
					Ok(next) => {
						self.inner
							.lock()
							.map_err(|_| eyre::eyre!("State lock poisoned."))?
							.lanes
							.insert(id, next.clone());
						return Ok(next);
					},
					Err(error) if error.to_string().contains("EpochMismatch") => continue,
					Err(error) => return Err(error),
				}
			}
			eyre::bail!("lane_compare_and_swap_retry_exhausted");
		}

		let mut state = self.inner.lock().map_err(|_| eyre::eyre!("State lock poisoned."))?;
		let current = state
			.lanes
			.get(&id)
			.cloned()
			.unwrap_or_else(|| LaneAggregate::new(id.clone(), binding_fingerprint));
		let next = transition(&current, current.epoch(), binding_fingerprint, command)
			.map_err(|rejection| eyre::eyre!("lane_transition_rejected:{rejection:?}"))?;
		state.lanes.insert(id, next.clone());
		Ok(next)
	}
}

#[cfg(test)]
mod tests {
	use tempfile::TempDir;

	use super::*;

	#[test]
	fn rejects_same_active_tracker_issue_across_projects() {
		let store = StateStore::open_in_memory().expect("store");
		let first = LaneId::new("first", "issue-1").expect("first lane");
		let second = LaneId::new("second", "issue-1").expect("second lane");
		store
			.transition_lane(
				first.clone(),
				0,
				"binding-1",
				LaneCommand::Admit { intake_authority_id: String::from("authority-1") },
			)
			.expect("first admit");
		store
			.transition_lane(
				second.clone(),
				0,
				"binding-2",
				LaneCommand::Admit { intake_authority_id: String::from("authority-2") },
			)
			.expect("second admit");
		store
			.transition_lane(
				first,
				1,
				"binding-1",
				LaneCommand::AcquireClaim { run_id: String::from("run-1") },
			)
			.expect("first claim");
		let error = store
			.transition_lane(
				second,
				1,
				"binding-2",
				LaneCommand::AcquireClaim { run_id: String::from("run-2") },
			)
			.expect_err("duplicate active issue must fail");
		assert!(error.to_string().contains("TrackerIssueAlreadyActive"));
	}

	#[test]
	fn stale_epoch_and_binding_fail_without_mutating_lane() {
		let store = StateStore::open_in_memory().expect("store");
		let id = LaneId::new("first", "issue-1").expect("lane");
		store
			.transition_lane(
				id.clone(),
				0,
				"binding-1",
				LaneCommand::Admit { intake_authority_id: String::from("authority-1") },
			)
			.expect("admit");
		store
			.transition_lane(
				id.clone(),
				1,
				"binding-1",
				LaneCommand::AcquireClaim { run_id: String::from("run-1") },
			)
			.expect("claim");
		assert!(store.transition_lane(id.clone(), 1, "binding-1", LaneCommand::BeginRun).is_err());
		assert!(store.transition_lane(id.clone(), 2, "binding-2", LaneCommand::BeginRun).is_err());
		assert_eq!(store.lane(&id).expect("read").expect("lane").epoch(), 2);
	}

	#[test]
	fn persistent_lane_round_trips_and_rejects_cross_process_stale_epoch() {
		let temp_dir = TempDir::new().expect("tempdir");
		let database = temp_dir.path().join("state.sqlite");
		let first = StateStore::open(&database).expect("first store");
		let stale = StateStore::open(&database).expect("stale store");
		let id = LaneId::new("first", "issue-1").expect("lane");
		first
			.transition_lane(
				id.clone(),
				0,
				"binding-1",
				LaneCommand::Admit { intake_authority_id: String::from("authority-1") },
			)
			.expect("admit");
		first
			.transition_lane(
				id.clone(),
				1,
				"binding-1",
				LaneCommand::AcquireClaim { run_id: String::from("run-1") },
			)
			.expect("claim");

		let error = stale
			.transition_lane(
				id.clone(),
				0,
				"binding-1",
				LaneCommand::AcquireClaim { run_id: String::from("run-2") },
			)
			.expect_err("stale insert must fail");
		assert!(error.to_string().contains("EpochMismatch"));

		let reopened = StateStore::open(&database).expect("reopened store");
		let lane = reopened.lane(&id).expect("read").expect("lane");
		assert_eq!(lane.epoch(), 2);
		assert_eq!(lane.intake_authority_id(), Some("authority-1"));
		assert_eq!(lane.claim_run_id(), Some("run-1"));
	}

	#[test]
	fn sqlite_constraint_rejects_same_active_issue_across_project_processes() {
		let temp_dir = TempDir::new().expect("tempdir");
		let database = temp_dir.path().join("state.sqlite");
		let first = StateStore::open(&database).expect("first store");
		let second = StateStore::open(&database).expect("second store");
		let first_id = LaneId::new("first", "issue-1").expect("lane");
		let second_id = LaneId::new("second", "issue-1").expect("lane");
		first
			.transition_lane(
				first_id.clone(),
				0,
				"binding-1",
				LaneCommand::Admit { intake_authority_id: String::from("authority-1") },
			)
			.expect("first admit");
		second
			.transition_lane(
				second_id.clone(),
				0,
				"binding-2",
				LaneCommand::Admit { intake_authority_id: String::from("authority-2") },
			)
			.expect("second admit");
		first
			.transition_lane(
				first_id,
				1,
				"binding-1",
				LaneCommand::AcquireClaim { run_id: String::from("run-1") },
			)
			.expect("first claim");
		let error = second
			.transition_lane(
				second_id,
				1,
				"binding-2",
				LaneCommand::AcquireClaim { run_id: String::from("run-2") },
			)
			.expect_err("cross-project active issue must fail");
		assert!(error.to_string().contains("TrackerIssueAlreadyActive"));
	}
}
