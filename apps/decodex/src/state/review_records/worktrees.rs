use std::path::PathBuf;

#[cfg(test)] use time::OffsetDateTime;

use crate::{
	lane_authority::{LaneCommand, LaneId},
	prelude::{Result, eyre},
	state::{
		StateStore, WorktreeMapping, runtime_records::WorktreeMappingRecord,
	},
};
#[cfg(test)] use crate::state::WORKTREE_PROVENANCE_RUNTIME_RECORDED;
#[cfg(test)] use crate::state::WORKTREE_PROVENANCE_RUNTIME_RECOVERED;

impl StateStore {
	/// Attach a worktree to an already claimed canonical lane, then update its legacy projection.
	pub fn upsert_claimed_worktree(
		&self,
		project_id: &str,
		issue_id: &str,
		branch_name: &str,
		worktree_path: &str,
	) -> Result<()> {
		let lane_id = LaneId::new(project_id, issue_id)?;
		#[cfg(test)]
		let existing_lane = self.lane(&lane_id)?;
		let binding = match self.registered_project_binding(project_id)? {
			Some(binding) => binding,
			None => {
				#[cfg(not(test))]
				eyre::bail!("Project is not registered; worktree attachment is forbidden.");
				#[cfg(test)]
				crate::lane_authority::ProjectBinding::new(
					project_id,
					"test-owner",
					"test-repository",
					"team-test",
					&format!("decodex:queued:{project_id}"),
					existing_lane
						.as_ref()
						.map_or_else(
							|| format!("test-binding:{project_id}"),
							|lane| lane.binding_fingerprint().to_owned(),
						)
						.as_str(),
				)?
			},
		};
		#[cfg(test)]
		if existing_lane.is_none()
			&& let Some(lease) = self.lease_for_issue(issue_id)?
			&& lease.project_id() == project_id
		{
			self.apply_lane_command(
				lane_id.clone(),
				binding.config_fingerprint(),
				LaneCommand::Admit {
					intake_authority_id: format!("test-intake-authority:{project_id}:{issue_id}"),
				},
			)?;
			self.apply_lane_command(
				lane_id.clone(),
				binding.config_fingerprint(),
				LaneCommand::AcquireClaim { run_id: lease.run_id().to_owned() },
			)?;
		}
		self.apply_lane_command(
			lane_id,
			binding.config_fingerprint(),
			LaneCommand::AttachWorktree {
				branch_name: branch_name.to_owned(),
				worktree_path: PathBuf::from(worktree_path),
			},
		)?;
		#[cfg(not(test))]
		return Ok(());
		#[cfg(test)]
		self.upsert_worktree(project_id, issue_id, branch_name, worktree_path)
	}

	/// Detach the exact worktree owned by a claimed canonical lane and clear its projection.
	pub fn detach_claimed_worktree(
		&self,
		project_id: &str,
		issue_id: &str,
		branch_name: &str,
		worktree_path: &str,
	) -> Result<()> {
		let lane_id = LaneId::new(project_id, issue_id)?;
		#[cfg(test)]
		let existing_lane = self.lane(&lane_id)?;
		let binding = match self.registered_project_binding(project_id)? {
			Some(binding) => binding,
			None => {
				#[cfg(not(test))]
				eyre::bail!("Project is not registered; worktree detachment is forbidden.");
				#[cfg(test)]
				crate::lane_authority::ProjectBinding::new(
					project_id,
					"test-owner",
					"test-repository",
					"team-test",
					&format!("decodex:queued:{project_id}"),
					existing_lane
						.as_ref()
						.map_or("test-binding", |lane| lane.binding_fingerprint()),
				)?
			},
		};
		self.apply_lane_command(
			lane_id,
			binding.config_fingerprint(),
			LaneCommand::DetachWorktree {
				branch_name: branch_name.to_owned(),
				worktree_path: PathBuf::from(worktree_path),
			},
		)?;
		self.clear_worktree_mapping(issue_id)
	}

	/// Create or replace the worktree mapping for one issue.
	#[cfg(test)]
	pub fn upsert_worktree(
		&self,
		project_id: &str,
		issue_id: &str,
		branch_name: &str,
		worktree_path: &str,
	) -> Result<()> {
		let mut state = self.lock_without_refresh()?;
		let now_unix = OffsetDateTime::now_utc().unix_timestamp();
		let created_at_unix = state
			.worktrees
			.get(issue_id)
			.and_then(|mapping| mapping.created_at_unix)
			.or(Some(now_unix));
		let mapping = WorktreeMappingRecord {
			project_id: project_id.to_owned(),
			issue_id: issue_id.to_owned(),
			branch_name: branch_name.to_owned(),
			worktree_path: PathBuf::from(worktree_path),
			provenance_source: WORKTREE_PROVENANCE_RUNTIME_RECORDED.to_owned(),
			created_at_unix,
			updated_at_unix: Some(now_unix),
		};

		state.worktrees.insert(issue_id.to_owned(), mapping.clone());
		state.remember_run_project(project_id, issue_id, None);

		self.upsert_worktree_and_remember_run_project_locked(&mapping)
	}

	/// Create or refresh a worktree mapping reconstructed from retained local state.
	pub(crate) fn upsert_recovered_worktree(
		&self,
		project_id: &str,
		issue_id: &str,
		branch_name: &str,
		worktree_path: &str,
		observed_at_unix: Option<i64>,
	) -> Result<()> {
		#[cfg(not(test))]
		let _ = observed_at_unix;
		let lane_id = LaneId::new(project_id, issue_id)?;
		let lane = self
			.lane(&lane_id)?
			.ok_or_else(|| eyre::eyre!("Recovered worktree requires canonical lane authority."))?;
		if lane.claim_run_id().is_none() {
			eyre::bail!("Recovered worktree requires an active canonical lane claim.");
		}
		self.apply_lane_command(
			lane_id,
			lane.binding_fingerprint(),
			LaneCommand::AttachWorktree {
				branch_name: branch_name.to_owned(),
				worktree_path: PathBuf::from(worktree_path),
			},
		)?;
		#[cfg(not(test))]
		return Ok(());

		#[cfg(test)]
		{
			let mut state = self.lock_without_refresh()?;
			let existing = state.worktrees.get(issue_id);
			let existing_provenance_source =
				existing.map(|mapping| mapping.provenance_source.as_str());
			let provenance_source = match existing_provenance_source {
				Some(WORKTREE_PROVENANCE_RUNTIME_RECORDED) => WORKTREE_PROVENANCE_RUNTIME_RECORDED,
				Some(WORKTREE_PROVENANCE_RUNTIME_RECOVERED) => WORKTREE_PROVENANCE_RUNTIME_RECOVERED,
				_ => WORKTREE_PROVENANCE_RUNTIME_RECOVERED,
			}
			.to_owned();
			let existing_created_at_unix = existing.and_then(|mapping| mapping.created_at_unix);
			let existing_updated_at_unix = existing.and_then(|mapping| mapping.updated_at_unix);
			let created_at_unix = existing_created_at_unix.or(observed_at_unix);
			let updated_at_unix = match (existing_updated_at_unix, observed_at_unix) {
				(Some(existing), Some(observed)) => Some(existing.max(observed)),
				(Some(existing), None) => Some(existing),
				(None, observed) => observed,
			};
			let mapping = WorktreeMappingRecord {
				project_id: project_id.to_owned(),
				issue_id: issue_id.to_owned(),
				branch_name: branch_name.to_owned(),
				worktree_path: PathBuf::from(worktree_path),
				provenance_source,
				created_at_unix,
				updated_at_unix,
			};

			state.worktrees.insert(issue_id.to_owned(), mapping.clone());
			state.remember_run_project(project_id, issue_id, None);

			self.upsert_worktree_and_remember_run_project_locked(&mapping)?;
			Ok(())
		}
	}

	/// Read the worktree mapping for one issue.
	pub fn worktree_for_issue(&self, issue_id: &str) -> Result<Option<WorktreeMapping>> {
		if let Some(sqlite) = &self.sqlite {
			let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;

			return sqlite
				.worktree_for_issue(issue_id)
				.map(|mapping| mapping.map(|mapping| mapping.as_public()));
		}

		let state = self.lock()?;

		Ok(state.worktrees.get(issue_id).map(WorktreeMappingRecord::as_public))
	}

	/// List all known worktree mappings.
	pub fn list_worktrees(&self, project_id: &str) -> Result<Vec<WorktreeMapping>> {
		let mut state = self.lock_without_refresh()?;

		self.refresh_project_run_metadata_state_locked(&mut state, project_id)?;

		let mut mappings = state
			.worktrees
			.values()
			.filter(|mapping| mapping.project_id == project_id)
			.map(WorktreeMappingRecord::as_public)
			.collect::<Vec<_>>();

		mappings.sort_by(|left, right| left.issue_id.cmp(&right.issue_id));

		Ok(mappings)
	}

	/// Remove the worktree mapping for one issue.
	pub fn clear_worktree(&self, issue_id: &str) -> Result<()> {
		let mut state = self.lock()?;

		state.worktrees.remove(issue_id);
		state
			.review_lifecycle_records
			.retain(|key, record| key.issue_id != issue_id || record.sequence > 0);
		state.review_policy_checkpoints.retain(|key, _record| key.issue_id != issue_id);
		self.persist_runtime_state_locked(&state)?;

		self.delete_worktree_and_review_ephemera_locked(issue_id)
	}

	/// Remove only the worktree mapping for one issue.
	pub(crate) fn clear_worktree_mapping(&self, issue_id: &str) -> Result<()> {
		let mut state = self.lock()?;

		state.worktrees.remove(issue_id);
		self.persist_runtime_state_locked(&state)?;

		self.delete_worktree_mapping_locked(issue_id)
	}
}
