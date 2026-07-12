use rusqlite::OptionalExtension;

use crate::{
	lane_authority::{LaneAggregate, LaneId, LanePhase, ProjectBinding},
	state::sqlite_store::{
		SqliteStateStore,
		queries::{
			self, ConnectorBackoff, IssueLease, PathBuf, ProjectRegistration, Result, StateData,
		},
	},
};
#[cfg(test)] use crate::state::WorktreeMappingRecord;

impl SqliteStateStore {
	pub(in crate::state) fn lane(&self, id: &LaneId) -> Result<Option<LaneAggregate>> {
		self.connection
			.query_row(
				"SELECT binding_fingerprint, epoch, phase, intake_authority_id, claim_run_id, branch_name, worktree_path \
				 FROM lanes WHERE project_key = ?1 AND tracker_issue_id = ?2",
				queries::params![id.project_key(), id.tracker_issue_id()],
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
			.optional()
			.map_err(Into::into)
	}

	pub(in crate::state) fn load_lanes(&self, state: &mut StateData) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_key, tracker_issue_id, binding_fingerprint, epoch, phase, \
			 intake_authority_id, claim_run_id, branch_name, worktree_path FROM lanes",
		)?;
		let rows = statement.query_map([], |row| {
			let project_key: String = row.get(0)?;
			let tracker_issue_id: String = row.get(1)?;
			let phase_value: String = row.get(4)?;
			let phase = LanePhase::from_str(&phase_value).ok_or_else(|| {
				rusqlite::Error::InvalidColumnType(
					4,
					String::from("phase"),
					rusqlite::types::Type::Text,
				)
			})?;
			let id = LaneId::new(&project_key, &tracker_issue_id).map_err(|_| {
				rusqlite::Error::InvalidColumnType(
					0,
					String::from("project_key"),
					rusqlite::types::Type::Text,
				)
			})?;
			let epoch = u64::try_from(row.get::<_, i64>(3)?)
				.map_err(|_| rusqlite::Error::IntegralValueOutOfRange(3, -1))?;
			let worktree_path = row.get::<_, Option<String>>(8)?.map(PathBuf::from);
			let aggregate = LaneAggregate::from_persisted_parts(
				id.clone(),
				row.get(2)?,
				epoch,
				phase,
				row.get(5)?,
				row.get(6)?,
				row.get(7)?,
				worktree_path,
			);
			Ok((id, aggregate))
		})?;

		for row in rows {
			let (id, aggregate) = row?;
			state.lanes.insert(id, aggregate);
		}
		Ok(())
	}

	pub(in crate::state) fn load_projects(&self, state: &mut StateData) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT service_id, config_path, repo_root, worktree_root, workflow_path, \
			 tracker_api_key_env_var, github_token_env_var, github_owner, github_repository, \
			 tracker_team_id, routing_label, enabled, config_fingerprint, \
			 updated_at, updated_at_unix FROM projects",
		)?;
		let rows = statement.query_map([], |row| {
			let service_id: String = row.get(0)?;
			let config_fingerprint: String = row.get(12)?;
			let binding = ProjectBinding::from_validated_parts(
				&service_id,
				&row.get::<_, String>(7)?,
				&row.get::<_, String>(8)?,
				&row.get::<_, String>(9)?,
				&row.get::<_, String>(10)?,
				&config_fingerprint,
			);

			Ok((
				service_id.clone(),
				ProjectRegistration {
					service_id,
					config_path: PathBuf::from(row.get::<_, String>(1)?),
					repo_root: PathBuf::from(row.get::<_, String>(2)?),
					worktree_root: PathBuf::from(row.get::<_, String>(3)?),
					workflow_path: PathBuf::from(row.get::<_, String>(4)?),
					tracker_api_key_env_var: row.get(5)?,
					github_token_env_var: row.get(6)?,
					enabled: row.get::<_, i64>(11)? != 0,
					config_fingerprint,
					binding,
					updated_at: row.get(13)?,
					updated_at_unix: row.get(14)?,
				},
			))
		})?;

		for row in rows {
			let (service_id, project) = row?;
			project.validate_binding()?;

			state.projects.insert(service_id, project);
		}

		Ok(())
	}

	pub(in crate::state) fn load_leases(&self, state: &mut StateData) -> Result<()> {
		let mut statement = self
			.connection
			.prepare("SELECT issue_id, project_id, run_id, issue_state FROM leases")?;
		let rows = statement.query_map([], |row| {
			let issue_id: String = row.get(0)?;

			Ok((
				issue_id.clone(),
				IssueLease {
					issue_id,
					project_id: row.get(1)?,
					run_id: row.get(2)?,
					issue_state: row.get(3)?,
				},
			))
		})?;

		for row in rows {
			let (issue_id, lease) = row?;

			state.leases.insert(issue_id, lease);
		}

		Ok(())
	}

	pub(in crate::state) fn load_worktrees(&self, state: &mut StateData) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT issue_id, project_id, branch_name, worktree_path,
					provenance_source, created_at_unix, updated_at_unix
				 FROM worktrees",
		)?;
		let rows = statement.query_map([], |row| {
			let mapping = queries::worktree_mapping_record_from_row(row)?;

			Ok((mapping.issue_id.clone(), mapping))
		})?;

		for row in rows {
			let (issue_id, mapping) = row?;

			state.worktrees.insert(issue_id, mapping);
		}

		Ok(())
	}

	#[cfg(test)]
	pub(in crate::state) fn worktree_for_issue(
		&self,
		issue_id: &str,
	) -> Result<Option<WorktreeMappingRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT issue_id, project_id, branch_name, worktree_path,
			 provenance_source, created_at_unix, updated_at_unix
			 FROM worktrees
			 WHERE issue_id = ?1
			 LIMIT 1",
		)?;
		let mut rows = statement.query(queries::params![issue_id])?;

		Ok(rows
			.next()?
			.map(crate::state::sqlite_store::queries::worktree_mapping_record_from_row)
			.transpose()?)
	}

	pub(in crate::state) fn load_connector_backoffs(&self, state: &mut StateData) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, connector, sync_phase, quota_class, reset_unix_epoch, \
			 reset_source, warning, updated_at, updated_at_unix FROM connector_backoffs",
		)?;
		let rows = statement.query_map([], |row| {
			let project_id: String = row.get(0)?;
			let connector: String = row.get(1)?;

			Ok((
				(project_id.clone(), connector.clone()),
				ConnectorBackoff {
					project_id,
					connector,
					sync_phase: row.get(2)?,
					quota_class: row.get(3)?,
					reset_unix_epoch: row.get(4)?,
					reset_source: row.get(5)?,
					warning: row.get(6)?,
					updated_at: row.get(7)?,
					updated_at_unix: row.get(8)?,
				},
			))
		})?;

		for row in rows {
			let (key, record) = row?;

			state.connector_backoffs.insert(key, record);
		}

		Ok(())
	}
}
