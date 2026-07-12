use std::{sync::atomic::{AtomicU64, Ordering}, time::{SystemTime, UNIX_EPOCH}};

use sha2::{Digest as _, Sha256};

#[cfg(test)] use crate::lane_authority::LaneTransitionRejection;
use crate::{
	lane_authority::{
		AuthorityDecision, AuthorityEventType, AuthorityReasonCode, AuthorityTransitionContext,
		LaneAggregate, LaneClaim, LaneCommand, LaneId, transition,
	},
	prelude::{Result, eyre},
	state::StateStore,
};

impl StateStore {
	pub(crate) fn claim_for_lane(
		&self,
		project_id: &str,
		issue_id: &str,
	) -> Result<Option<LaneClaim>> {
		let lane_id = LaneId::new(project_id, issue_id)?;
		Ok(self.lane(&lane_id)?.as_ref().and_then(LaneClaim::from_lane))
	}

	pub(crate) fn list_lane_claims(&self, project_id: &str) -> Result<Vec<LaneClaim>> {
		let state = self
			.inner
			.lock()
			.map_err(|_| eyre::eyre!("State lock poisoned."))?;
		let mut claims = state
			.lanes
			.values()
			.filter(|lane| lane.id().project_key() == project_id)
			.filter_map(LaneClaim::from_lane)
			.collect::<Vec<_>>();
		claims.sort_by(|left, right| {
			left.id().tracker_issue_id().cmp(right.id().tracker_issue_id())
		});
		Ok(claims)
	}

	pub(crate) fn release_lane_claim(
		&self,
		project_id: &str,
		issue_id: &str,
		expected_run_id: &str,
	) -> Result<bool> {
		let Some(claim) = self.claim_for_lane(project_id, issue_id)? else {
			return Ok(false);
		};
		if claim.run_id() != expected_run_id {
			return Ok(false);
		}
		self.clear_lease(issue_id)?;
		if self.claim_for_lane(project_id, issue_id)?.is_some() {
			let lane_id = LaneId::new(project_id, issue_id)?;
			let lane = self.lane(&lane_id)?.ok_or_else(|| eyre::eyre!("Lane disappeared."))?;
			self.apply_lane_command(
				lane_id,
				lane.binding_fingerprint(),
				LaneCommand::ReleaseClaim { run_id: expected_run_id.to_owned() },
			)?;
		}
		Ok(true)
	}

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
				let authority_event = self
					.authority_transition_context(&id, expected_epoch, &command)?
					.map(|context| context.into_lane_event(&id, binding_fingerprint))
					.transpose()?;
				match sqlite.transition_lane(
					&id,
					expected_epoch,
					binding_fingerprint,
					command.clone(),
					authority_event,
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

	fn authority_transition_context(
		&self,
		id: &LaneId,
		expected_epoch: u64,
		command: &LaneCommand,
	) -> Result<Option<AuthorityTransitionContext>> {
		static EVENT_ORDINAL: AtomicU64 = AtomicU64::new(1);
		let Some(invocation) = self.invocation_identity.as_ref() else {
			return Ok(None);
		};
		let ordinal = EVENT_ORDINAL.fetch_add(1, Ordering::Relaxed);
		let recorded_at_unix_micros = i64::try_from(
			SystemTime::now().duration_since(UNIX_EPOCH)?.as_micros(),
		)?;
		let facts = lane_command_fingerprint(id, expected_epoch, command);
		let event_id = format!(
			"event:{}",
			hex_sha256(
				format!("{}:{ordinal}:{facts}", invocation.invocation_id()).as_bytes()
			),
		);
		let event_type = match command {
			LaneCommand::ReleaseClaim { .. } => AuthorityEventType::LaneReleased,
			LaneCommand::BeginSupersededCleanup => AuthorityEventType::LaneTransferred,
			_ => AuthorityEventType::TransitionCommitted,
		};
		Ok(Some(AuthorityTransitionContext {
			invocation: invocation.clone(),
			event_id,
			event_type,
			transition_id: format!("lane-transition:{}:{expected_epoch}", id.tracker_issue_id()),
			correlation_id: invocation.invocation_id().to_owned(),
			causation_id: invocation.invocation_id().to_owned(),
			observed_facts_fingerprint: facts,
			decision: AuthorityDecision::Committed,
			reason_codes: vec![AuthorityReasonCode::BindingMatched],
			operation_id: None,
			runtime_version: concat!(env!("CARGO_PKG_VERSION"), "-", env!("VERGEN_GIT_SHA"))
				.to_owned(),
			recorded_at_unix_micros,
			boot_id_fingerprint: hex_sha256(
				crate::state::current_host_boot_id().unwrap_or_else(|| String::from("unavailable")).as_bytes(),
			),
			monotonic_nanos: ordinal,
		}))
	}
}

fn lane_command_fingerprint(id: &LaneId, expected_epoch: u64, command: &LaneCommand) -> String {
	let mut digest = Sha256::new();
	for field in [
		b"decodex.lane-command/1".as_slice(),
		id.project_key().as_bytes(),
		id.tracker_issue_id().as_bytes(),
		&expected_epoch.to_be_bytes(),
	] {
		digest.update(u64::try_from(field.len()).unwrap_or(u64::MAX).to_be_bytes());
		digest.update(field);
	}
	match command {
		LaneCommand::Admit { intake_authority_id } =>
			update_command_digest(&mut digest, "admit", &[intake_authority_id.as_bytes()]),
		LaneCommand::AcquireClaim { run_id } =>
			update_command_digest(&mut digest, "acquire_claim", &[run_id.as_bytes()]),
		LaneCommand::FreezeAdmittedBase { oid } =>
			update_command_digest(&mut digest, "freeze_admitted_base", &[oid.as_bytes()]),
		LaneCommand::ReleaseClaim { run_id } =>
			update_command_digest(&mut digest, "release_claim", &[run_id.as_bytes()]),
		LaneCommand::AttachWorktree { branch_name, worktree_path } =>
			update_worktree_command_digest(
				&mut digest,
				"attach_worktree",
				branch_name,
				worktree_path,
			),
		LaneCommand::DetachWorktree { branch_name, worktree_path } =>
			update_worktree_command_digest(
				&mut digest,
				"detach_worktree",
				branch_name,
				worktree_path,
			),
		LaneCommand::BeginRun => update_command_digest(&mut digest, "begin_run", &[]),
		LaneCommand::BeginReview => update_command_digest(&mut digest, "begin_review", &[]),
		LaneCommand::Land => update_command_digest(&mut digest, "land", &[]),
		LaneCommand::Cancel => update_command_digest(&mut digest, "cancel", &[]),
		LaneCommand::RequireAttention =>
			update_command_digest(&mut digest, "require_attention", &[]),
		LaneCommand::BeginSupersededCleanup =>
			update_command_digest(&mut digest, "begin_superseded_cleanup", &[]),
		LaneCommand::CompleteTerminalCleanup =>
			update_command_digest(&mut digest, "complete_terminal_cleanup", &[]),
	}
	digest.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
}

fn update_command_digest(digest: &mut Sha256, kind: &str, fields: &[&[u8]]) {
	for field in std::iter::once(kind.as_bytes()).chain(fields.iter().copied()) {
		digest.update(u64::try_from(field.len()).unwrap_or(u64::MAX).to_be_bytes());
		digest.update(field);
	}
}

fn update_worktree_command_digest(
	digest: &mut Sha256,
	kind: &str,
	branch_name: &str,
	worktree_path: &std::path::Path,
) {
	let path = path_bytes(worktree_path);
	update_command_digest(digest, kind, &[branch_name.as_bytes(), &path]);
}

#[cfg(unix)]
fn path_bytes(path: &std::path::Path) -> Vec<u8> {
	use std::os::unix::ffi::OsStrExt as _;
	path.as_os_str().as_bytes().to_vec()
}

#[cfg(not(unix))]
fn path_bytes(path: &std::path::Path) -> Vec<u8> {
	path.to_string_lossy().as_bytes().to_vec()
}

fn hex_sha256(bytes: &[u8]) -> String {
	Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
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

	#[test]
	fn broker_owned_store_appends_event_for_production_lane_writer() {
		let temp_dir = TempDir::new().expect("tempdir");
		let database = temp_dir.path().join("state.sqlite");
		let store = StateStore::open_with_invocation(
			&database,
			crate::authority_broker::test_invocation_identity(),
		)
		.expect("store");
		store.initialize_authority_generation(1, &[7_u8; 32]).expect("generation");
		let id = LaneId::new("pubfi", "PUB-1711").expect("lane");
		store
			.apply_lane_command(
				id.clone(),
				"binding-1",
				LaneCommand::Admit { intake_authority_id: String::from("authority-1") },
			)
			.expect("admit");
		let events = store.verify_authority_events().expect("events");
		assert_eq!(events.len(), 1);
		assert_eq!(events[0].draft.project_key.as_deref(), Some("pubfi"));
		assert_eq!(events[0].draft.tracker_issue_id.as_deref(), Some("PUB-1711"));
		assert_eq!(events[0].draft.event_type, AuthorityEventType::TransitionCommitted);
		assert_eq!(events[0].draft.decision, AuthorityDecision::Committed);
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
		first
			.transition_lane(
				id.clone(),
				2,
				"binding-1",
				LaneCommand::FreezeAdmittedBase {
					oid: String::from("0123456789abcdef0123456789abcdef01234567"),
				},
			)
			.expect("freeze admitted base");

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
		assert_eq!(lane.epoch(), 3);
		assert_eq!(lane.intake_authority_id(), Some("authority-1"));
		assert_eq!(lane.claim_run_id(), Some("run-1"));
		assert_eq!(lane.admitted_base_oid(), Some("0123456789abcdef0123456789abcdef01234567"));
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
