use super::{
	AutonomyObjectiveRuntimeRecord, AutonomyProposalRuntimeRecord, AutonomySignalRuntimeRecord,
	DecisionContractRuntimeRecord, ExecutionProgramRuntimeRecord, ProgramIntakePlanKey,
	ProgramIntakePlanRecord, ProgramIssueMappingKey, ProgramIssueMappingRecord, Result, StateData,
	autonomy_objective_record_from_row_parts, autonomy_objective_runtime_row_parts,
	autonomy_proposal_record_from_row_parts, autonomy_proposal_runtime_row_parts,
	autonomy_signal_record_from_row_parts, autonomy_signal_runtime_row_parts,
	decision_contract_record_from_row_parts, decision_contract_runtime_row_parts,
	execution_program_record_from_row_parts, execution_program_runtime_row_parts, eyre, params,
	program_intake_plan_row, program_issue_mapping_row,
};

impl super::super::SqliteStateStore {
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
}
