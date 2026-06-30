use super::{
	AutonomyObjectiveRuntimeRecord, AutonomyProposalRuntimeRecord, AutonomySignalRuntimeRecord,
	ConnectorBackoff, DecisionContractRuntimeRecord, EvidenceArtifactKey,
	EvidenceArtifactRuntimeRecord, ExecutionProgramRuntimeRecord, IssueLease,
	LinearExecutionEventRecord, LinearExecutionEventRuntimeRecord, LoopGuardrailKey,
	LoopGuardrailRuntimeRecord, OptionalExtension, PathBuf, PrivateExecutionEventRuntimeRecord,
	ProgramIntakePlanKey, ProgramIntakePlanRecord, ProgramIssueMappingKey,
	ProgramIssueMappingRecord, ProjectRegistration, ProtocolEventSummaryRecord, Result,
	ReviewLifecycleKey, ReviewLifecycleRuntimeRecord, ReviewPolicyKey, ReviewPolicyRuntimeRecord,
	Row, RunAttemptRecord, RunControlChannelRecord, StateData, Value, WorktreeMappingRecord,
	autonomy_objective_record_from_row_parts, autonomy_objective_runtime_row_parts,
	autonomy_proposal_record_from_row_parts, autonomy_proposal_runtime_row_parts,
	autonomy_signal_record_from_row_parts, autonomy_signal_runtime_row_parts,
	decision_contract_record_from_row_parts, decision_contract_runtime_row_parts,
	execution_program_record_from_row_parts, execution_program_runtime_row_parts, eyre, params,
	program_intake_plan_row, program_issue_mapping_row, run_activity_summary_record_from_row,
	run_attempt_record_from_row, timestamp_parts, worktree_mapping_record_from_row,
};

impl super::SqliteStateStore {
	pub(in crate::state) fn load_state(&self) -> Result<StateData> {
		let mut state = StateData::default();

		self.load_projects(&mut state)?;
		self.load_leases(&mut state)?;
		self.load_run_attempts(&mut state)?;
		self.load_run_control_channels(&mut state)?;
		self.load_protocol_event_summaries(&mut state)?;
		self.load_run_activity_summaries(&mut state)?;
		self.load_worktrees(&mut state)?;
		self.load_linear_execution_events(&mut state)?;
		self.load_private_execution_events(&mut state)?;
		self.load_decision_contracts(&mut state)?;
		self.load_autonomy_objectives(&mut state)?;
		self.load_autonomy_signals(&mut state)?;
		self.load_autonomy_proposals(&mut state)?;
		self.load_execution_programs(&mut state)?;
		self.load_program_intake_state(&mut state)?;
		self.load_review_lifecycle_records(&mut state)?;
		self.load_review_policy_checkpoints(&mut state)?;
		self.load_evidence_artifacts(&mut state)?;
		self.load_loop_guardrail_checkpoints(&mut state)?;
		self.load_connector_backoffs(&mut state)?;

		Ok(state)
	}

	pub(in crate::state) fn load_project_run_metadata_for_project(
		&self,
		project_id: &str,
	) -> Result<StateData> {
		let mut state = StateData::default();

		self.load_leases(&mut state)?;
		self.load_run_attempts_for_project(&mut state, project_id)?;
		self.load_run_activity_summaries_for_loaded_runs(&mut state)?;
		self.load_worktrees(&mut state)?;
		self.load_run_control_channels_for_project(&mut state, project_id)?;

		Ok(state)
	}

	pub(in crate::state) fn load_project_loop_evidence_for_project(
		&self,
		project_id: &str,
	) -> Result<StateData> {
		let mut state = StateData::default();

		self.load_private_execution_events_for_project(&mut state, project_id)?;
		self.load_review_lifecycle_records_for_project(&mut state, project_id)?;
		self.load_review_policy_checkpoints_for_project(&mut state, project_id)?;
		self.load_evidence_artifacts_for_project(&mut state, project_id)?;
		self.load_autonomy_signals_for_project(&mut state, project_id)?;
		self.load_autonomy_proposals_for_project(&mut state, project_id)?;

		Ok(state)
	}

	pub(in crate::state) fn load_project_registry_state(&self) -> Result<StateData> {
		let mut state = StateData::default();

		self.load_projects(&mut state)?;

		Ok(state)
	}

	pub(in crate::state) fn load_projects(&self, state: &mut StateData) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT service_id, config_path, repo_root, worktree_root, workflow_path, \
			 tracker_api_key_env_var, github_token_env_var, enabled, config_fingerprint, \
			 updated_at, updated_at_unix FROM projects",
		)?;
		let rows = statement.query_map([], |row| {
			let service_id: String = row.get(0)?;

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
					enabled: row.get::<_, i64>(7)? != 0,
					config_fingerprint: row.get(8)?,
					updated_at: row.get(9)?,
					updated_at_unix: row.get(10)?,
				},
			))
		})?;

		for row in rows {
			let (service_id, project) = row?;

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

	pub(in crate::state) fn load_run_attempts(&self, state: &mut StateData) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT run_id, project_id, issue_id, attempt_number, status, thread_id, turn_id, \
			 updated_at, updated_at_unix FROM run_attempts",
		)?;
		let rows = statement.query_map([], |row| {
			let run_id: String = row.get(0)?;

			Ok((
				run_id.clone(),
				RunAttemptRecord {
					run_id,
					project_id: row.get(1)?,
					issue_id: row.get(2)?,
					attempt_number: row.get(3)?,
					status: row.get(4)?,
					thread_id: row.get(5)?,
					turn_id: row.get(6)?,
					updated_at: row.get(7)?,
					updated_at_unix: row.get(8)?,
				},
			))
		})?;

		for row in rows {
			let (run_id, attempt) = row?;

			state.run_attempts.insert(run_id, attempt);
		}

		Ok(())
	}

	pub(in crate::state) fn load_run_attempts_for_project(
		&self,
		state: &mut StateData,
		project_id: &str,
	) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT run_id, project_id, issue_id, attempt_number, status, thread_id, turn_id, \
			 updated_at, updated_at_unix FROM run_attempts \
			 WHERE project_id = ?1",
		)?;
		let rows = statement.query_map(params![project_id], run_attempt_record_from_row)?;

		for row in rows {
			let attempt = row?;

			state.run_attempts.insert(attempt.run_id.clone(), attempt);
		}

		Ok(())
	}

	pub(in crate::state) fn load_run_control_channels(&self, state: &mut StateData) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT run_id, project_id, issue_id, attempt_number, transport, channel_path, status, \
			 published_at, published_at_unix, updated_at, updated_at_unix \
			 FROM run_control_channels",
		)?;
		let rows = statement.query_map([], |row| {
			Ok(RunControlChannelRecord {
				run_id: row.get(0)?,
				project_id: row.get(1)?,
				issue_id: row.get(2)?,
				attempt_number: row.get(3)?,
				transport: row.get(4)?,
				channel_path: PathBuf::from(row.get::<_, String>(5)?),
				status: row.get(6)?,
				published_at: row.get(7)?,
				published_at_unix: row.get(8)?,
				updated_at: row.get(9)?,
				updated_at_unix: row.get(10)?,
			})
		})?;

		for row in rows {
			let channel = row?;

			state.control_channels.insert(channel.run_id.clone(), channel);
		}

		Ok(())
	}

	pub(in crate::state) fn load_run_control_channels_for_project(
		&self,
		state: &mut StateData,
		project_id: &str,
	) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT run_id, project_id, issue_id, attempt_number, transport, channel_path, status, \
			 published_at, published_at_unix, updated_at, updated_at_unix \
			 FROM run_control_channels WHERE project_id = ?1",
		)?;
		let rows = statement.query_map(params![project_id], |row| {
			Ok(RunControlChannelRecord {
				run_id: row.get(0)?,
				project_id: row.get(1)?,
				issue_id: row.get(2)?,
				attempt_number: row.get(3)?,
				transport: row.get(4)?,
				channel_path: PathBuf::from(row.get::<_, String>(5)?),
				status: row.get(6)?,
				published_at: row.get(7)?,
				published_at_unix: row.get(8)?,
				updated_at: row.get(9)?,
				updated_at_unix: row.get(10)?,
			})
		})?;

		for row in rows {
			let channel = row?;

			state.control_channels.insert(channel.run_id.clone(), channel);
		}

		Ok(())
	}

	pub(in crate::state) fn retry_budget_attempt_count(&self, issue_id: &str) -> Result<i64> {
		self.connection
			.query_row(
				"SELECT COUNT(*) FROM run_attempts \
				 WHERE issue_id = ?1 AND status IN ('failed', 'interrupted', 'terminal_guarded')",
				params![issue_id],
				|row| row.get(0),
			)
			.map_err(Into::into)
	}

	pub(in crate::state) fn issue_has_retry_budget_attempt_after(
		&self,
		issue_id: &str,
		attempt_number: i64,
	) -> Result<bool> {
		let count = self.connection.query_row(
			"SELECT COUNT(*) FROM run_attempts \
			 WHERE issue_id = ?1 \
			 AND attempt_number > ?2 \
			 AND status IN ('failed', 'interrupted', 'terminal_guarded') \
			 LIMIT 1",
			params![issue_id, attempt_number],
			|row| row.get::<_, i64>(0),
		)?;

		Ok(count > 0)
	}

	pub(in crate::state) fn run_attempt_for_issue_attempt(
		&self,
		issue_id: &str,
		attempt_number: i64,
	) -> Result<Option<RunAttemptRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT run_id, project_id, issue_id, attempt_number, status, thread_id, turn_id, \
			 updated_at, updated_at_unix FROM run_attempts \
			 WHERE issue_id = ?1 AND attempt_number = ?2 \
			 ORDER BY updated_at_unix DESC, run_id DESC \
			 LIMIT 1",
		)?;
		let mut rows = statement.query(params![issue_id, attempt_number])?;

		Ok(rows.next()?.map(run_attempt_record_from_row).transpose()?)
	}

	pub(in crate::state) fn latest_run_attempt_for_issue(
		&self,
		issue_id: &str,
	) -> Result<Option<RunAttemptRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT run_id, project_id, issue_id, attempt_number, status, thread_id, turn_id, \
			 updated_at, updated_at_unix FROM run_attempts \
			 WHERE issue_id = ?1 \
			 ORDER BY attempt_number DESC, updated_at_unix DESC, run_id DESC \
			 LIMIT 1",
		)?;
		let mut rows = statement.query(params![issue_id])?;

		Ok(rows.next()?.map(run_attempt_record_from_row).transpose()?)
	}

	pub(in crate::state) fn list_run_attempts_for_issue(
		&self,
		issue_id: &str,
	) -> Result<Vec<RunAttemptRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT run_id, project_id, issue_id, attempt_number, status, thread_id, turn_id, \
			 updated_at, updated_at_unix FROM run_attempts \
			 WHERE issue_id = ?1 \
			 ORDER BY attempt_number ASC, run_id ASC",
		)?;
		let rows = statement.query_map(params![issue_id], run_attempt_record_from_row)?;

		rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
	}

	pub(in crate::state) fn list_run_attempts_for_project(
		&self,
		project_id: &str,
	) -> Result<Vec<RunAttemptRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT run_id, project_id, issue_id, attempt_number, status, thread_id, turn_id, \
			 updated_at, updated_at_unix FROM run_attempts \
			 WHERE project_id = ?1 \
			 ORDER BY updated_at_unix DESC, run_id ASC",
		)?;
		let rows = statement.query_map(params![project_id], run_attempt_record_from_row)?;

		rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
	}

	pub(in crate::state) fn run_has_protocol_event(
		&self,
		run_id: &str,
		event_type: &str,
	) -> Result<bool> {
		let exists = self.connection.query_row(
			"SELECT EXISTS(
			 SELECT 1 FROM protocol_events
			 WHERE run_id = ?1 AND event_type = ?2
			 LIMIT 1
			 )",
			params![run_id, event_type],
			|row| row.get::<_, i64>(0),
		)?;

		Ok(exists != 0)
	}

	pub(in crate::state) fn load_protocol_event_summaries(
		&self,
		state: &mut StateData,
	) -> Result<()> {
		self.load_compacted_protocol_event_summaries(state)
	}

	pub(in crate::state) fn load_protocol_event_summaries_for_runs(
		&self,
		state: &mut StateData,
		run_ids: &[String],
	) -> Result<()> {
		for run_id in run_ids {
			state.event_summaries.remove(run_id);
			if !self.load_compacted_protocol_event_summary_for_run(state, run_id)? {
				self.load_protocol_event_summary_for_run(state, run_id)?;
			}
		}

		Ok(())
	}

	pub(in crate::state) fn rebuild_protocol_event_summaries_for_runs(
		&self,
		state: &mut StateData,
		run_ids: &[String],
	) -> Result<()> {
		for run_id in run_ids {
			state.event_summaries.remove(run_id);
			self.load_protocol_event_summary_for_run(state, run_id)?;
		}

		Ok(())
	}

	pub(in crate::state) fn load_protocol_event_summary_for_run(
		&self,
		state: &mut StateData,
		run_id: &str,
	) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT totals.event_count, totals.last_sequence_number, last.event_type, \
			 last.created_at, last.created_at_unix \
			 FROM (
			 SELECT COUNT(*) AS event_count, MAX(sequence_number) AS last_sequence_number \
			 FROM protocol_events WHERE run_id = ?1
			 ) totals \
			 JOIN protocol_events last \
			 ON last.run_id = ?1 \
			 AND last.sequence_number = totals.last_sequence_number",
		)?;
		let summary = statement
			.query_row(params![run_id], |row| {
				Ok(ProtocolEventSummaryRecord {
					event_count: row.get(0)?,
					last_sequence_number: Some(row.get(1)?),
					last_event_type: Some(row.get(2)?),
					last_event_at: Some(row.get(3)?),
					last_event_at_unix: Some(row.get(4)?),
				})
			})
			.optional()?;

		if let Some(summary) = summary {
			self.upsert_protocol_event_summary(run_id, &summary)?;
			state.event_summaries.insert(run_id.to_owned(), summary);
		}

		Ok(())
	}

	pub(in crate::state) fn load_run_activity_summaries(
		&self,
		state: &mut StateData,
	) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT run_id, attempt_number, child_agent_activity_json, protocol_activity_json,
			 updated_at, updated_at_unix FROM run_activity_summaries ORDER BY run_id",
		)?;
		let rows = statement.query_map([], run_activity_summary_record_from_row)?;

		for row in rows {
			let summary = row?;

			state.run_activity_summaries.insert(summary.run_id.clone(), summary);
		}

		Ok(())
	}

	pub(in crate::state) fn load_run_activity_summaries_for_loaded_runs(
		&self,
		state: &mut StateData,
	) -> Result<()> {
		let run_ids = state.run_attempts.keys().cloned().collect::<Vec<_>>();

		self.load_run_activity_summaries_for_runs(state, &run_ids)
	}

	pub(in crate::state) fn load_run_activity_summaries_for_runs(
		&self,
		state: &mut StateData,
		run_ids: &[String],
	) -> Result<()> {
		for run_id in run_ids {
			self.load_run_activity_summary_for_run(state, run_id)?;
		}

		Ok(())
	}

	pub(in crate::state) fn load_run_activity_summary_for_run(
		&self,
		state: &mut StateData,
		run_id: &str,
	) -> Result<()> {
		state.run_activity_summaries.remove(run_id);

		let mut statement = self.connection.prepare(
			"SELECT run_id, attempt_number, child_agent_activity_json, protocol_activity_json,
			 updated_at, updated_at_unix FROM run_activity_summaries WHERE run_id = ?1",
		)?;
		let summary = statement
			.query_row(params![run_id], run_activity_summary_record_from_row)
			.optional()?;

		if let Some(summary) = summary {
			state.run_activity_summaries.insert(run_id.to_owned(), summary);
		}

		Ok(())
	}

	pub(in crate::state) fn load_compacted_protocol_event_summaries(
		&self,
		state: &mut StateData,
	) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT run_id, event_count, last_sequence_number, last_event_type, last_event_at, \
			 last_event_at_unix FROM protocol_event_summaries ORDER BY run_id",
		)?;
		let rows = statement.query_map([], |row| {
			Ok((
				row.get::<_, String>(0)?,
				ProtocolEventSummaryRecord {
					event_count: row.get(1)?,
					last_sequence_number: row.get(2)?,
					last_event_type: row.get(3)?,
					last_event_at: row.get(4)?,
					last_event_at_unix: row.get(5)?,
				},
			))
		})?;

		for row in rows {
			let (run_id, summary) = row?;

			state.event_summaries.insert(run_id, summary);
		}

		Ok(())
	}

	pub(in crate::state) fn load_compacted_protocol_event_summary_for_run(
		&self,
		state: &mut StateData,
		run_id: &str,
	) -> Result<bool> {
		let mut statement = self.connection.prepare(
			"SELECT event_count, last_sequence_number, last_event_type, last_event_at, \
			 last_event_at_unix FROM protocol_event_summaries WHERE run_id = ?1",
		)?;
		let summary = statement
			.query_row(params![run_id], |row| {
				Ok(ProtocolEventSummaryRecord {
					event_count: row.get(0)?,
					last_sequence_number: row.get(1)?,
					last_event_type: row.get(2)?,
					last_event_at: row.get(3)?,
					last_event_at_unix: row.get(4)?,
				})
			})
			.optional()?;

		if let Some(summary) = summary {
			state.event_summaries.insert(run_id.to_owned(), summary);

			return Ok(true);
		}

		Ok(false)
	}

	pub(in crate::state) fn upsert_protocol_event_summary(
		&self,
		run_id: &str,
		summary: &ProtocolEventSummaryRecord,
	) -> Result<()> {
		let now = timestamp_parts();

		self.connection.execute(
			"INSERT OR REPLACE INTO protocol_event_summaries (
					run_id, event_count, last_sequence_number, last_event_type, last_event_at,
					last_event_at_unix, compacted_at, compacted_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
			params![
				run_id,
				summary.event_count,
				summary.last_sequence_number,
				summary.last_event_type.as_deref(),
				summary.last_event_at.as_deref(),
				summary.last_event_at_unix,
				now.text,
				now.unix,
			],
		)?;

		Ok(())
	}

	pub(in crate::state) fn load_worktrees(&self, state: &mut StateData) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT issue_id, project_id, branch_name, worktree_path,
					provenance_source, created_at_unix, updated_at_unix
				 FROM worktrees",
		)?;
		let rows = statement.query_map([], |row| {
			let mapping = worktree_mapping_record_from_row(row)?;

			Ok((mapping.issue_id.clone(), mapping))
		})?;

		for row in rows {
			let (issue_id, mapping) = row?;

			state.worktrees.insert(issue_id, mapping);
		}

		Ok(())
	}

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
		let mut rows = statement.query(params![issue_id])?;

		Ok(rows.next()?.map(worktree_mapping_record_from_row).transpose()?)
	}

	pub(in crate::state) fn load_linear_execution_events(
		&self,
		state: &mut StateData,
	) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT payload_json, event_unix, recorded_at, recorded_at_unix \
			 FROM linear_execution_events",
		)?;
		let rows = statement.query_map([], |row| {
			Ok((
				row.get::<_, String>(0)?,
				row.get::<_, Option<i64>>(1)?,
				row.get::<_, String>(2)?,
				row.get::<_, i64>(3)?,
			))
		})?;

		for row in rows {
			let (payload_json, event_unix, recorded_at, recorded_at_unix) = row?;
			let record = serde_json::from_str::<LinearExecutionEventRecord>(&payload_json)?;
			let record = LinearExecutionEventRuntimeRecord {
				record,
				event_unix,
				recorded_at,
				recorded_at_unix,
			};

			state.linear_execution_events.insert(record.record.idempotency_key.clone(), record);
		}

		Ok(())
	}

	pub(in crate::state) fn list_linear_execution_events(
		&self,
		service_id: &str,
		issue_id: &str,
	) -> Result<Vec<LinearExecutionEventRuntimeRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT payload_json, event_unix, recorded_at, recorded_at_unix \
			 FROM linear_execution_events \
			 WHERE service_id = ?1 AND issue_id = ?2",
		)?;
		let rows = statement.query_map(params![service_id, issue_id], |row| {
			Ok((
				row.get::<_, String>(0)?,
				row.get::<_, Option<i64>>(1)?,
				row.get::<_, String>(2)?,
				row.get::<_, i64>(3)?,
			))
		})?;
		let mut records = Vec::new();

		for row in rows {
			let (payload_json, event_unix, recorded_at, recorded_at_unix) = row?;
			let record = serde_json::from_str::<LinearExecutionEventRecord>(&payload_json)?;

			records.push(LinearExecutionEventRuntimeRecord {
				record,
				event_unix,
				recorded_at,
				recorded_at_unix,
			});
		}

		Ok(records)
	}

	pub(in crate::state) fn load_private_execution_events(
		&self,
		state: &mut StateData,
	) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT record_id, project_id, issue_id, run_id, attempt_number, event_type, \
			 payload_json, recorded_at, recorded_at_unix \
			 FROM private_execution_events \
			 ORDER BY record_id ASC",
		)?;
		let rows = statement.query_map([], |row| {
			Ok((
				row.get::<_, i64>(0)?,
				row.get::<_, String>(1)?,
				row.get::<_, String>(2)?,
				row.get::<_, String>(3)?,
				row.get::<_, i64>(4)?,
				row.get::<_, String>(5)?,
				row.get::<_, String>(6)?,
				row.get::<_, String>(7)?,
				row.get::<_, i64>(8)?,
			))
		})?;

		for row in rows {
			let (
				record_id,
				project_id,
				issue_id,
				run_id,
				attempt_number,
				event_type,
				payload_json,
				recorded_at,
				recorded_at_unix,
			) = row?;
			let payload = serde_json::from_str::<Value>(&payload_json)?;

			state.private_execution_events.push(PrivateExecutionEventRuntimeRecord {
				record_id,
				project_id,
				issue_id,
				run_id,
				attempt_number,
				event_type,
				payload,
				recorded_at,
				recorded_at_unix,
			});
		}

		Ok(())
	}

	pub(in crate::state) fn load_private_execution_events_for_project(
		&self,
		state: &mut StateData,
		project_id: &str,
	) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT record_id, project_id, issue_id, run_id, attempt_number, event_type, \
			 payload_json, recorded_at, recorded_at_unix \
			 FROM private_execution_events \
			 WHERE project_id = ?1 \
			 ORDER BY record_id ASC",
		)?;
		let rows = statement.query_map(params![project_id], |row| {
			Ok((
				row.get::<_, i64>(0)?,
				row.get::<_, String>(1)?,
				row.get::<_, String>(2)?,
				row.get::<_, String>(3)?,
				row.get::<_, i64>(4)?,
				row.get::<_, String>(5)?,
				row.get::<_, String>(6)?,
				row.get::<_, String>(7)?,
				row.get::<_, i64>(8)?,
			))
		})?;

		for row in rows {
			let (
				record_id,
				project_id,
				issue_id,
				run_id,
				attempt_number,
				event_type,
				payload_json,
				recorded_at,
				recorded_at_unix,
			) = row?;
			let payload = serde_json::from_str::<Value>(&payload_json)?;

			state.private_execution_events.push(PrivateExecutionEventRuntimeRecord {
				record_id,
				project_id,
				issue_id,
				run_id,
				attempt_number,
				event_type,
				payload,
				recorded_at,
				recorded_at_unix,
			});
		}

		Ok(())
	}

	pub(in crate::state) fn load_decision_contracts(&self, state: &mut StateData) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, contract_id, source_issue_id, status, payload_json, created_at, \
			 created_at_unix, updated_at, updated_at_unix \
			 FROM decision_contracts \
			 ORDER BY project_id ASC, contract_id ASC",
		)?;
		let rows = statement.query_map([], decision_contract_runtime_row_parts)?;

		for row in rows {
			let record = decision_contract_record_from_row_parts(row?)?;

			state.decision_contracts.insert(record.key(), record);
		}

		Ok(())
	}

	pub(in crate::state) fn decision_contract(
		&self,
		project_id: &str,
		contract_id: &str,
	) -> Result<Option<DecisionContractRuntimeRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, contract_id, source_issue_id, status, payload_json, created_at, \
			 created_at_unix, updated_at, updated_at_unix \
			 FROM decision_contracts \
			 WHERE project_id = ?1 AND contract_id = ?2 \
			 LIMIT 1",
		)?;
		let mut rows = statement.query(params![project_id, contract_id])?;

		rows.next()?
			.map(decision_contract_runtime_row_parts)
			.transpose()?
			.map(decision_contract_record_from_row_parts)
			.transpose()
	}

	pub(in crate::state) fn list_decision_contracts_for_issue(
		&self,
		project_id: &str,
		source_issue_id: &str,
	) -> Result<Vec<DecisionContractRuntimeRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, contract_id, source_issue_id, status, payload_json, created_at, \
			 created_at_unix, updated_at, updated_at_unix \
			 FROM decision_contracts \
			 WHERE project_id = ?1 AND source_issue_id = ?2 \
			 ORDER BY created_at_unix ASC, contract_id ASC",
		)?;
		let rows = statement
			.query_map(params![project_id, source_issue_id], decision_contract_runtime_row_parts)?;
		let mut records = Vec::new();

		for row in rows {
			records.push(decision_contract_record_from_row_parts(row?)?);
		}

		Ok(records)
	}

	pub(in crate::state) fn list_decision_contracts_for_project(
		&self,
		project_id: &str,
	) -> Result<Vec<DecisionContractRuntimeRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, contract_id, source_issue_id, status, payload_json, created_at, \
			 created_at_unix, updated_at, updated_at_unix \
			 FROM decision_contracts \
			 WHERE project_id = ?1 \
			 ORDER BY created_at_unix ASC, contract_id ASC",
		)?;
		let rows = statement.query_map(params![project_id], decision_contract_runtime_row_parts)?;
		let mut records = Vec::new();

		for row in rows {
			records.push(decision_contract_record_from_row_parts(row?)?);
		}

		Ok(records)
	}

	pub(in crate::state) fn load_autonomy_objectives(&self, state: &mut StateData) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, objective_id, version, state, payload_json, created_at, \
			 created_at_unix, updated_at, updated_at_unix \
			 FROM autonomy_objectives \
			 ORDER BY project_id ASC, objective_id ASC, version ASC",
		)?;
		let rows = statement.query_map([], autonomy_objective_runtime_row_parts)?;

		for row in rows {
			let record = autonomy_objective_record_from_row_parts(row?)?;

			state.autonomy_objectives.insert(record.key(), record);
		}

		Ok(())
	}

	pub(in crate::state) fn autonomy_objective(
		&self,
		project_id: &str,
		objective_id: &str,
		version: u64,
	) -> Result<Option<AutonomyObjectiveRuntimeRecord>> {
		let version = i64::try_from(version)
			.map_err(|_| eyre::eyre!("Autonomy objective version exceeds SQLite integer range."))?;
		let mut statement = self.connection.prepare(
			"SELECT project_id, objective_id, version, state, payload_json, created_at, \
			 created_at_unix, updated_at, updated_at_unix \
			 FROM autonomy_objectives \
			 WHERE project_id = ?1 AND objective_id = ?2 AND version = ?3 \
			 LIMIT 1",
		)?;
		let mut rows = statement.query(params![project_id, objective_id, version])?;

		rows.next()?
			.map(autonomy_objective_runtime_row_parts)
			.transpose()?
			.map(autonomy_objective_record_from_row_parts)
			.transpose()
	}

	pub(in crate::state) fn current_accepted_autonomy_objective(
		&self,
		project_id: &str,
		objective_id: &str,
	) -> Result<Option<AutonomyObjectiveRuntimeRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, objective_id, version, state, payload_json, created_at, \
			 created_at_unix, updated_at, updated_at_unix \
			 FROM autonomy_objectives \
			 WHERE project_id = ?1 AND objective_id = ?2 AND state = 'accepted' \
			 ORDER BY version DESC \
			 LIMIT 1",
		)?;
		let mut rows = statement.query(params![project_id, objective_id])?;

		rows.next()?
			.map(autonomy_objective_runtime_row_parts)
			.transpose()?
			.map(autonomy_objective_record_from_row_parts)
			.transpose()
	}

	pub(in crate::state) fn list_autonomy_objective_history(
		&self,
		project_id: &str,
		objective_id: &str,
	) -> Result<Vec<AutonomyObjectiveRuntimeRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, objective_id, version, state, payload_json, created_at, \
			 created_at_unix, updated_at, updated_at_unix \
			 FROM autonomy_objectives \
			 WHERE project_id = ?1 AND objective_id = ?2 \
			 ORDER BY version ASC",
		)?;
		let rows = statement
			.query_map(params![project_id, objective_id], autonomy_objective_runtime_row_parts)?;
		let mut records = Vec::new();

		for row in rows {
			records.push(autonomy_objective_record_from_row_parts(row?)?);
		}

		Ok(records)
	}

	pub(in crate::state) fn recent_autonomy_objectives_for_project(
		&self,
		project_id: &str,
		limit: usize,
	) -> Result<Vec<AutonomyObjectiveRuntimeRecord>> {
		let limit = i64::try_from(limit).unwrap_or(i64::MAX);
		let mut statement = self.connection.prepare(
			"SELECT project_id, objective_id, version, state, payload_json, created_at, created_at_unix, updated_at, updated_at_unix \
			 FROM autonomy_objectives \
			 WHERE project_id = ?1 \
			 ORDER BY updated_at_unix DESC, objective_id ASC, version ASC \
			 LIMIT ?2",
		)?;
		let rows = statement
			.query_map(params![project_id, limit], autonomy_objective_runtime_row_parts)?;
		let mut records = Vec::new();

		for row in rows {
			records.push(autonomy_objective_record_from_row_parts(row?)?);
		}

		Ok(records)
	}

	pub(in crate::state) fn load_autonomy_signals(&self, state: &mut StateData) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, signal_id, objective_id, objective_version, kind, fingerprint, \
			 freshness, evidence_class, confidence, privacy, payload_json, created_at, \
			 created_at_unix, updated_at, updated_at_unix \
			 FROM autonomy_signals \
			 ORDER BY project_id ASC, objective_id ASC, objective_version ASC, updated_at_unix ASC",
		)?;
		let rows = statement.query_map([], autonomy_signal_runtime_row_parts)?;

		for row in rows {
			let record = autonomy_signal_record_from_row_parts(row?)?;

			state.autonomy_signals.insert(record.key(), record);
		}

		Ok(())
	}

	pub(in crate::state) fn load_autonomy_signals_for_project(
		&self,
		state: &mut StateData,
		project_id: &str,
	) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, signal_id, objective_id, objective_version, kind, fingerprint, \
			 freshness, evidence_class, confidence, privacy, payload_json, created_at, \
			 created_at_unix, updated_at, updated_at_unix \
			 FROM autonomy_signals \
			 WHERE project_id = ?1 \
			 ORDER BY updated_at_unix DESC, signal_id ASC",
		)?;
		let rows = statement.query_map(params![project_id], autonomy_signal_runtime_row_parts)?;

		for row in rows {
			let record = autonomy_signal_record_from_row_parts(row?)?;

			state.autonomy_signals.insert(record.key(), record);
		}

		Ok(())
	}

	pub(in crate::state) fn autonomy_signal(
		&self,
		project_id: &str,
		signal_id: &str,
	) -> Result<Option<AutonomySignalRuntimeRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, signal_id, objective_id, objective_version, kind, fingerprint, \
			 freshness, evidence_class, confidence, privacy, payload_json, created_at, \
			 created_at_unix, updated_at, updated_at_unix \
			 FROM autonomy_signals \
			 WHERE project_id = ?1 AND signal_id = ?2 \
			 LIMIT 1",
		)?;
		let mut rows = statement.query(params![project_id, signal_id])?;

		rows.next()?
			.map(autonomy_signal_runtime_row_parts)
			.transpose()?
			.map(autonomy_signal_record_from_row_parts)
			.transpose()
	}

	pub(in crate::state) fn list_autonomy_signals_for_objective(
		&self,
		project_id: &str,
		objective_id: &str,
		objective_version: u64,
	) -> Result<Vec<AutonomySignalRuntimeRecord>> {
		let version = i64::try_from(objective_version).map_err(|_| {
			eyre::eyre!("Autonomy signal objective_version exceeds SQLite integer range.")
		})?;
		let mut statement = self.connection.prepare(
			"SELECT project_id, signal_id, objective_id, objective_version, kind, fingerprint, \
			 freshness, evidence_class, confidence, privacy, payload_json, created_at, \
			 created_at_unix, updated_at, updated_at_unix \
			 FROM autonomy_signals \
			 WHERE project_id = ?1 AND objective_id = ?2 AND objective_version = ?3 \
			 ORDER BY updated_at_unix ASC, signal_id ASC",
		)?;
		let rows = statement.query_map(
			params![project_id, objective_id, version],
			autonomy_signal_runtime_row_parts,
		)?;
		let mut records = Vec::new();

		for row in rows {
			records.push(autonomy_signal_record_from_row_parts(row?)?);
		}

		Ok(records)
	}

	pub(in crate::state) fn recent_autonomy_signals_for_project(
		&self,
		project_id: &str,
		limit: usize,
	) -> Result<Vec<AutonomySignalRuntimeRecord>> {
		let limit = i64::try_from(limit).map_err(|_| {
			eyre::eyre!("Autonomy signal readback limit exceeds SQLite integer range.")
		})?;
		let mut statement = self.connection.prepare(
			"SELECT project_id, signal_id, objective_id, objective_version, kind, fingerprint, \
			 freshness, evidence_class, confidence, privacy, payload_json, created_at, \
			 created_at_unix, updated_at, updated_at_unix \
			 FROM autonomy_signals \
			 WHERE project_id = ?1 \
			 ORDER BY updated_at_unix DESC, signal_id ASC \
			 LIMIT ?2",
		)?;
		let rows =
			statement.query_map(params![project_id, limit], autonomy_signal_runtime_row_parts)?;
		let mut records = Vec::new();

		for row in rows {
			records.push(autonomy_signal_record_from_row_parts(row?)?);
		}

		Ok(records)
	}

	pub(in crate::state) fn load_autonomy_proposals(&self, state: &mut StateData) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, proposal_id, objective_id, objective_version, state, fingerprint, \
			 source_family, intended_surface, payload_json, created_at, created_at_unix, \
			 updated_at, updated_at_unix \
			 FROM autonomy_proposals \
			 ORDER BY project_id ASC, objective_id ASC, objective_version ASC, updated_at_unix ASC",
		)?;
		let rows = statement.query_map([], autonomy_proposal_runtime_row_parts)?;

		for row in rows {
			let record = autonomy_proposal_record_from_row_parts(row?)?;

			state.autonomy_proposals.insert(record.key(), record);
		}

		Ok(())
	}

	pub(in crate::state) fn load_autonomy_proposals_for_project(
		&self,
		state: &mut StateData,
		project_id: &str,
	) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, proposal_id, objective_id, objective_version, state, fingerprint, \
			 source_family, intended_surface, payload_json, created_at, created_at_unix, \
			 updated_at, updated_at_unix \
			 FROM autonomy_proposals \
			 WHERE project_id = ?1 \
			 ORDER BY updated_at_unix DESC, proposal_id ASC",
		)?;
		let rows = statement.query_map(params![project_id], autonomy_proposal_runtime_row_parts)?;

		for row in rows {
			let record = autonomy_proposal_record_from_row_parts(row?)?;

			state.autonomy_proposals.insert(record.key(), record);
		}

		Ok(())
	}

	pub(in crate::state) fn autonomy_proposal(
		&self,
		project_id: &str,
		proposal_id: &str,
	) -> Result<Option<AutonomyProposalRuntimeRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, proposal_id, objective_id, objective_version, state, fingerprint, \
			 source_family, intended_surface, payload_json, created_at, created_at_unix, \
			 updated_at, updated_at_unix \
			 FROM autonomy_proposals \
			 WHERE project_id = ?1 AND proposal_id = ?2 \
			 LIMIT 1",
		)?;
		let mut rows = statement.query(params![project_id, proposal_id])?;

		rows.next()?
			.map(autonomy_proposal_runtime_row_parts)
			.transpose()?
			.map(autonomy_proposal_record_from_row_parts)
			.transpose()
	}

	pub(in crate::state) fn list_autonomy_proposals_for_objective(
		&self,
		project_id: &str,
		objective_id: &str,
		objective_version: u64,
	) -> Result<Vec<AutonomyProposalRuntimeRecord>> {
		let version = i64::try_from(objective_version).map_err(|_| {
			eyre::eyre!("Autonomy proposal objective_version exceeds SQLite integer range.")
		})?;
		let mut statement = self.connection.prepare(
			"SELECT project_id, proposal_id, objective_id, objective_version, state, fingerprint, \
			 source_family, intended_surface, payload_json, created_at, created_at_unix, \
			 updated_at, updated_at_unix \
			 FROM autonomy_proposals \
			 WHERE project_id = ?1 AND objective_id = ?2 AND objective_version = ?3 \
			 ORDER BY updated_at_unix ASC, proposal_id ASC",
		)?;
		let rows = statement.query_map(
			params![project_id, objective_id, version],
			autonomy_proposal_runtime_row_parts,
		)?;
		let mut records = Vec::new();

		for row in rows {
			records.push(autonomy_proposal_record_from_row_parts(row?)?);
		}

		Ok(records)
	}

	pub(in crate::state) fn recent_autonomy_proposals_for_project(
		&self,
		project_id: &str,
		limit: usize,
	) -> Result<Vec<AutonomyProposalRuntimeRecord>> {
		let limit = i64::try_from(limit).map_err(|_| {
			eyre::eyre!("Autonomy proposal readback limit exceeds SQLite integer range.")
		})?;
		let mut statement = self.connection.prepare(
			"SELECT project_id, proposal_id, objective_id, objective_version, state, fingerprint, \
			 source_family, intended_surface, payload_json, created_at, created_at_unix, \
			 updated_at, updated_at_unix \
			 FROM autonomy_proposals \
			 WHERE project_id = ?1 \
			 ORDER BY updated_at_unix DESC, proposal_id ASC \
			 LIMIT ?2",
		)?;
		let rows =
			statement.query_map(params![project_id, limit], autonomy_proposal_runtime_row_parts)?;
		let mut records = Vec::new();

		for row in rows {
			records.push(autonomy_proposal_record_from_row_parts(row?)?);
		}

		Ok(records)
	}

	pub(in crate::state) fn load_execution_programs(&self, state: &mut StateData) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, program_id, source_contract_id, payload_json, created_at, \
			 created_at_unix, updated_at, updated_at_unix \
			 FROM execution_programs \
			 ORDER BY project_id ASC, program_id ASC",
		)?;
		let rows = statement.query_map([], execution_program_runtime_row_parts)?;

		for row in rows {
			let record = execution_program_record_from_row_parts(row?)?;

			state.execution_programs.insert(record.key(), record);
		}

		Ok(())
	}

	pub(in crate::state) fn execution_program(
		&self,
		project_id: &str,
		program_id: &str,
	) -> Result<Option<ExecutionProgramRuntimeRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, program_id, source_contract_id, payload_json, created_at, \
			 created_at_unix, updated_at, updated_at_unix \
			 FROM execution_programs \
			 WHERE project_id = ?1 AND program_id = ?2 \
			 LIMIT 1",
		)?;
		let mut rows = statement.query(params![project_id, program_id])?;

		rows.next()?
			.map(execution_program_runtime_row_parts)
			.transpose()?
			.map(execution_program_record_from_row_parts)
			.transpose()
	}

	pub(in crate::state) fn list_execution_programs_for_contract(
		&self,
		project_id: &str,
		source_contract_id: &str,
	) -> Result<Vec<ExecutionProgramRuntimeRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, program_id, source_contract_id, payload_json, created_at, \
			 created_at_unix, updated_at, updated_at_unix \
			 FROM execution_programs \
			 WHERE project_id = ?1 AND source_contract_id = ?2 \
			 ORDER BY created_at_unix ASC, program_id ASC",
		)?;
		let rows = statement.query_map(
			params![project_id, source_contract_id],
			execution_program_runtime_row_parts,
		)?;
		let mut records = Vec::new();

		for row in rows {
			records.push(execution_program_record_from_row_parts(row?)?);
		}

		Ok(records)
	}

	pub(in crate::state) fn list_execution_programs(
		&self,
		project_id: &str,
	) -> Result<Vec<ExecutionProgramRuntimeRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, program_id, source_contract_id, payload_json, created_at, \
			 created_at_unix, updated_at, updated_at_unix \
			 FROM execution_programs \
			 WHERE project_id = ?1 \
			 ORDER BY created_at_unix ASC, program_id ASC",
		)?;
		let rows = statement.query_map(params![project_id], execution_program_runtime_row_parts)?;
		let mut records = Vec::new();

		for row in rows {
			records.push(execution_program_record_from_row_parts(row?)?);
		}

		Ok(records)
	}

	pub(in crate::state) fn load_program_intake_state(&self, state: &mut StateData) -> Result<()> {
		for record in self.list_all_program_intake_plans()? {
			state.program_intake_plans.insert(
				ProgramIntakePlanKey::new(&record.project_id, &record.program_id, &record.plan_id),
				record,
			);
		}
		for record in self.list_all_program_issue_mappings()? {
			state.program_issue_mappings.insert(
				ProgramIssueMappingKey::new(
					&record.project_id,
					&record.program_id,
					&record.node_id,
				),
				record,
			);
		}

		Ok(())
	}

	pub(in crate::state) fn list_all_program_intake_plans(
		&self,
	) -> Result<Vec<ProgramIntakePlanRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, program_id, plan_id, intake_kind, source_contract_id, \
			 accepted_contract_fingerprint, public_summary, created_at, created_at_unix, \
			 updated_at, updated_at_unix \
			 FROM program_intake_plans \
			 ORDER BY project_id ASC, program_id ASC, plan_id ASC",
		)?;
		let rows = statement.query_map([], program_intake_plan_row)?;
		let mut records = Vec::new();

		for row in rows {
			records.push(row?);
		}

		Ok(records)
	}

	pub(in crate::state) fn list_program_intake_plans(
		&self,
		project_id: &str,
	) -> Result<Vec<ProgramIntakePlanRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, program_id, plan_id, intake_kind, source_contract_id, \
			 accepted_contract_fingerprint, public_summary, created_at, created_at_unix, \
			 updated_at, updated_at_unix \
			 FROM program_intake_plans \
			 WHERE project_id = ?1 \
			 ORDER BY updated_at_unix ASC, program_id ASC, plan_id ASC",
		)?;
		let rows = statement.query_map(params![project_id], program_intake_plan_row)?;
		let mut records = Vec::new();

		for row in rows {
			records.push(row?);
		}

		Ok(records)
	}

	pub(in crate::state) fn list_all_program_issue_mappings(
		&self,
	) -> Result<Vec<ProgramIssueMappingRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, program_id, node_id, issue_id, issue_identifier, issue_state, \
			 queue_intent, has_active_label, has_opt_out_label, has_needs_attention_label, \
			 has_generic_dispatch_briefing, created_at, created_at_unix, updated_at, \
			 updated_at_unix \
			 FROM program_issue_mappings \
			 ORDER BY project_id ASC, program_id ASC, node_id ASC",
		)?;
		let rows = statement.query_map([], program_issue_mapping_row)?;
		let mut records = Vec::new();

		for row in rows {
			records.push(row?);
		}

		Ok(records)
	}

	pub(in crate::state) fn list_program_issue_mappings(
		&self,
		project_id: &str,
		program_id: &str,
	) -> Result<Vec<ProgramIssueMappingRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, program_id, node_id, issue_id, issue_identifier, issue_state, \
			 queue_intent, has_active_label, has_opt_out_label, has_needs_attention_label, \
			 has_generic_dispatch_briefing, created_at, created_at_unix, updated_at, \
			 updated_at_unix \
			 FROM program_issue_mappings \
			 WHERE project_id = ?1 AND program_id = ?2 \
			 ORDER BY updated_at_unix ASC, node_id ASC",
		)?;
		let rows =
			statement.query_map(params![project_id, program_id], program_issue_mapping_row)?;
		let mut records = Vec::new();

		for row in rows {
			records.push(row?);
		}

		Ok(records)
	}

	pub(in crate::state) fn load_review_lifecycle_records(
		&self,
		state: &mut StateData,
	) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, issue_id, branch_name, run_id, attempt_number, pr_url, \
			 target_base_ref_name, pr_head_ref_name, pr_head_oid, head_sha, phase, \
			 request_comment_database_id, request_created_at_unix_epoch, \
			 request_description_thumbs_up_count, request_retry_count, external_round_count, \
			 auto_merge_enabled_at_unix_epoch, landing_state, closeout_state, \
			 repair_attempt_count, evidence_json, next_action, updated_at, updated_at_unix \
			 FROM review_lifecycle_records",
		)?;
		let rows = statement.query_map([], |row| {
			let project_id: String = row.get(0)?;
			let issue_id: String = row.get(1)?;
			let branch_name: String = row.get(2)?;
			let run_id: String = row.get(3)?;
			let attempt_number: i64 = row.get(4)?;
			let request_description_thumbs_up_count =
				row.get::<_, Option<i64>>(13)?.and_then(|count| usize::try_from(count).ok());

			Ok((
				ReviewLifecycleKey::new(&project_id, &issue_id, &branch_name),
				ReviewLifecycleRuntimeRecord {
					project_id,
					issue_id,
					branch_name,
					run_id,
					attempt_number,
					pr_url: row.get(5)?,
					target_base_ref_name: row.get(6)?,
					pr_head_ref_name: row.get(7)?,
					pr_head_oid: row.get(8)?,
					head_sha: row.get(9)?,
					phase: row.get(10)?,
					request_comment_database_id: row.get(11)?,
					request_created_at_unix_epoch: row.get(12)?,
					request_description_thumbs_up_count,
					request_retry_count: row.get(14)?,
					external_round_count: row.get(15)?,
					auto_merge_enabled_at_unix_epoch: row.get(16)?,
					landing_state: row.get(17)?,
					closeout_state: row.get(18)?,
					repair_attempt_count: row.get(19)?,
					evidence_json: row.get(20)?,
					next_action: row.get(21)?,
					updated_at: row.get(22)?,
					updated_at_unix: row.get(23)?,
				},
			))
		})?;

		for row in rows {
			let (key, record) = row?;

			state.review_lifecycle_records.insert(key, record);
		}

		Ok(())
	}

	pub(in crate::state) fn load_review_lifecycle_records_for_project(
		&self,
		state: &mut StateData,
		project_id: &str,
	) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, issue_id, branch_name, run_id, attempt_number, pr_url, \
			 target_base_ref_name, pr_head_ref_name, pr_head_oid, head_sha, phase, \
			 request_comment_database_id, request_created_at_unix_epoch, \
			 request_description_thumbs_up_count, request_retry_count, external_round_count, \
			 auto_merge_enabled_at_unix_epoch, landing_state, closeout_state, \
			 repair_attempt_count, evidence_json, next_action, updated_at, updated_at_unix \
			 FROM review_lifecycle_records WHERE project_id = ?1",
		)?;
		let rows = statement.query_map(params![project_id], |row| {
			let project_id: String = row.get(0)?;
			let issue_id: String = row.get(1)?;
			let branch_name: String = row.get(2)?;
			let run_id: String = row.get(3)?;
			let attempt_number: i64 = row.get(4)?;
			let request_description_thumbs_up_count =
				row.get::<_, Option<i64>>(13)?.and_then(|count| usize::try_from(count).ok());

			Ok((
				ReviewLifecycleKey::new(&project_id, &issue_id, &branch_name),
				ReviewLifecycleRuntimeRecord {
					project_id,
					issue_id,
					branch_name,
					run_id,
					attempt_number,
					pr_url: row.get(5)?,
					target_base_ref_name: row.get(6)?,
					pr_head_ref_name: row.get(7)?,
					pr_head_oid: row.get(8)?,
					head_sha: row.get(9)?,
					phase: row.get(10)?,
					request_comment_database_id: row.get(11)?,
					request_created_at_unix_epoch: row.get(12)?,
					request_description_thumbs_up_count,
					request_retry_count: row.get(14)?,
					external_round_count: row.get(15)?,
					auto_merge_enabled_at_unix_epoch: row.get(16)?,
					landing_state: row.get(17)?,
					closeout_state: row.get(18)?,
					repair_attempt_count: row.get(19)?,
					evidence_json: row.get(20)?,
					next_action: row.get(21)?,
					updated_at: row.get(22)?,
					updated_at_unix: row.get(23)?,
				},
			))
		})?;

		for row in rows {
			let (key, record) = row?;

			state.review_lifecycle_records.insert(key, record);
		}

		Ok(())
	}

	pub(in crate::state) fn load_review_policy_checkpoints(
		&self,
		state: &mut StateData,
	) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, issue_id, run_id, attempt_number, phase, status, head_sha, \
			 nonclean_rounds, details_json, updated_at, updated_at_unix FROM review_policy_checkpoints",
		)?;
		let rows = statement.query_map([], |row| {
			let project_id: String = row.get(0)?;
			let issue_id: String = row.get(1)?;
			let run_id: String = row.get(2)?;
			let attempt_number: i64 = row.get(3)?;
			let phase: String = row.get(4)?;

			Ok((
				ReviewPolicyKey::new(&project_id, &issue_id, &run_id, attempt_number, &phase),
				ReviewPolicyRuntimeRecord {
					project_id,
					issue_id,
					run_id,
					attempt_number,
					phase,
					status: row.get(5)?,
					head_sha: row.get(6)?,
					nonclean_rounds: row.get(7)?,
					details_json: row.get(8)?,
					updated_at: row.get(9)?,
					updated_at_unix: row.get(10)?,
				},
			))
		})?;

		for row in rows {
			let (key, record) = row?;

			state.review_policy_checkpoints.insert(key, record);
		}

		Ok(())
	}

	pub(in crate::state) fn load_review_policy_checkpoints_for_project(
		&self,
		state: &mut StateData,
		project_id: &str,
	) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, issue_id, run_id, attempt_number, phase, status, head_sha, \
			 nonclean_rounds, details_json, updated_at, updated_at_unix FROM review_policy_checkpoints \
			 WHERE project_id = ?1",
		)?;
		let rows = statement.query_map(params![project_id], |row| {
			let project_id: String = row.get(0)?;
			let issue_id: String = row.get(1)?;
			let run_id: String = row.get(2)?;
			let attempt_number: i64 = row.get(3)?;
			let phase: String = row.get(4)?;

			Ok((
				ReviewPolicyKey::new(&project_id, &issue_id, &run_id, attempt_number, &phase),
				ReviewPolicyRuntimeRecord {
					project_id,
					issue_id,
					run_id,
					attempt_number,
					phase,
					status: row.get(5)?,
					head_sha: row.get(6)?,
					nonclean_rounds: row.get(7)?,
					details_json: row.get(8)?,
					updated_at: row.get(9)?,
					updated_at_unix: row.get(10)?,
				},
			))
		})?;

		for row in rows {
			let (key, record) = row?;

			state.review_policy_checkpoints.insert(key, record);
		}

		Ok(())
	}

	pub(in crate::state) fn load_evidence_artifacts(&self, state: &mut StateData) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, issue_id, artifact_kind, key_hash, phase, status, head_sha, \
			 key_json, payload_json, source_run_id, source_attempt_number, updated_at, \
			 updated_at_unix FROM evidence_artifacts",
		)?;
		let rows = statement.query_map([], Self::evidence_artifact_from_row)?;

		for row in rows {
			let (key, record) = row?;

			state.evidence_artifacts.insert(key, record);
		}

		Ok(())
	}

	pub(in crate::state) fn load_evidence_artifacts_for_project(
		&self,
		state: &mut StateData,
		project_id: &str,
	) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, issue_id, artifact_kind, key_hash, phase, status, head_sha, \
			 key_json, payload_json, source_run_id, source_attempt_number, updated_at, \
			 updated_at_unix FROM evidence_artifacts WHERE project_id = ?1",
		)?;
		let rows = statement.query_map(params![project_id], Self::evidence_artifact_from_row)?;

		for row in rows {
			let (key, record) = row?;

			state.evidence_artifacts.insert(key, record);
		}

		Ok(())
	}

	pub(in crate::state) fn evidence_artifact_from_row(
		row: &Row<'_>,
	) -> rusqlite::Result<(EvidenceArtifactKey, EvidenceArtifactRuntimeRecord)> {
		let project_id: String = row.get(0)?;
		let issue_id: String = row.get(1)?;
		let artifact_kind: String = row.get(2)?;
		let key_hash: String = row.get(3)?;

		Ok((
			EvidenceArtifactKey::new(&project_id, &issue_id, &artifact_kind, &key_hash),
			EvidenceArtifactRuntimeRecord {
				project_id,
				issue_id,
				artifact_kind,
				key_hash,
				phase: row.get(4)?,
				status: row.get(5)?,
				head_sha: row.get(6)?,
				key_json: row.get(7)?,
				payload_json: row.get(8)?,
				source_run_id: row.get(9)?,
				source_attempt_number: row.get(10)?,
				updated_at: row.get(11)?,
				updated_at_unix: row.get(12)?,
			},
		))
	}

	pub(in crate::state) fn load_loop_guardrail_checkpoints(
		&self,
		state: &mut StateData,
	) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, issue_id, reason, fingerprint, run_id, attempt_number, \
			 consecutive_count, details_json, updated_at, updated_at_unix \
			 FROM loop_guardrail_checkpoints",
		)?;
		let rows = statement.query_map([], |row| {
			let project_id: String = row.get(0)?;
			let issue_id: String = row.get(1)?;
			let reason: String = row.get(2)?;

			Ok((
				LoopGuardrailKey::new(&project_id, &issue_id, &reason),
				LoopGuardrailRuntimeRecord {
					project_id,
					issue_id,
					reason,
					fingerprint: row.get(3)?,
					run_id: row.get(4)?,
					attempt_number: row.get(5)?,
					consecutive_count: row.get(6)?,
					details_json: row.get(7)?,
					updated_at: row.get(8)?,
					updated_at_unix: row.get(9)?,
				},
			))
		})?;

		for row in rows {
			let (key, record) = row?;

			state.loop_guardrail_checkpoints.insert(key, record);
		}

		Ok(())
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
