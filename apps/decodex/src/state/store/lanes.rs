#[cfg(test)] use crate::lane_authority::LaneTransitionRejection;
use crate::{
	lane_authority::{LaneAggregate, LaneCommand, LaneId, transition},
	prelude::{Result, eyre},
	state::StateStore,
};

impl StateStore {
	#[cfg_attr(not(test), allow(dead_code))]
	pub(crate) fn transition_lane_with_authority(
		&self,
		id: LaneId,
		expected_epoch: u64,
		binding_fingerprint: &str,
		command: LaneCommand,
		context: crate::lane_authority::AuthorityTransitionContext,
	) -> Result<LaneAggregate> {
		let event = context.into_lane_event(&id, binding_fingerprint)?;
		let sqlite = self
			.sqlite
			.as_ref()
			.ok_or_else(|| eyre::eyre!("Authority transitions require a persistent StateStore."))?;
		let next = sqlite
			.lock()
			.map_err(|_| eyre::eyre!("SQLite state lock poisoned."))?
			.transition_lane(&id, expected_epoch, binding_fingerprint, command, Some(event))?;
		self.inner
			.lock()
			.map_err(|_| eyre::eyre!("State lock poisoned."))?
			.lanes
			.insert(id, next.clone());
		Ok(next)
	}

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
				.transition_lane(&id, expected_epoch, binding_fingerprint, command, None)?;
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
					None,
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
	use crate::lane_authority::{
		AuthorityDecision, AuthorityEventType, AuthorityReasonCode, AuthorityTransitionContext,
	};

	#[test]
	fn lane_authority_v2_c5_lane_and_event_commit_or_rollback_together() {
		let temp_dir = TempDir::new().expect("tempdir");
		let database = temp_dir.path().join("state.sqlite");
		let store = StateStore::open(&database).expect("store");
		let id = LaneId::new("pubfi", "PUB-1711").expect("lane");
		let command = LaneCommand::Admit { intake_authority_id: String::from("authority-1") };
		assert!(
			store
				.transition_lane_with_authority(
					id.clone(),
					0,
					"binding-1",
					command.clone(),
					authority_context("event-without-generation"),
				)
				.is_err()
		);
		assert!(store.lane(&id).expect("lane read").is_none());
		store.initialize_authority_generation(1, &[5_u8; 32]).expect("generation");
		let lane = store
			.transition_lane_with_authority(
				id.clone(),
				0,
				"binding-1",
				command,
				authority_context("event-1"),
			)
			.expect("atomic authority transition");
		assert_eq!(lane.epoch(), 1);
		let events = store.verify_authority_events().expect("event chain");
		assert_eq!(events.len(), 1);
		assert_eq!(events[0].draft.event_id, "event-1");
	}

	fn authority_context(event_id: &str) -> AuthorityTransitionContext {
		AuthorityTransitionContext {
			invocation: crate::authority_broker::test_invocation_identity(),
			event_id: event_id.to_owned(),
			event_type: AuthorityEventType::TransitionCommitted,
			transition_id: String::from("transition-1"),
			correlation_id: String::from("correlation-1"),
			causation_id: String::from("causation-1"),
			observed_facts_fingerprint: String::from("facts-1"),
			decision: AuthorityDecision::Committed,
			reason_codes: vec![AuthorityReasonCode::BindingMatched],
			operation_id: None,
			runtime_version: String::from("0.2.0"),
			recorded_at_unix_micros: 1,
			boot_id_fingerprint: String::from("boot-1"),
			monotonic_nanos: 1,
		}
	}

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
