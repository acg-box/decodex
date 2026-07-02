use crate::{
	prelude::Result,
	state::{
		ProgramIntakePlanRecord, ProgramIssueMappingRecord, StateData,
		runtime_records::{ProgramIntakePlanKey, ProgramIssueMappingKey},
		runtime_row_parsers,
		sqlite_store::SqliteStateStore,
	},
};

impl SqliteStateStore {
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
		let rows = statement.query_map([], runtime_row_parsers::program_intake_plan_row)?;
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
		let rows = statement.query_map(
			rusqlite::params![project_id],
			runtime_row_parsers::program_intake_plan_row,
		)?;
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
		let rows = statement.query_map([], runtime_row_parsers::program_issue_mapping_row)?;
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
		let rows = statement.query_map(
			rusqlite::params![project_id, program_id],
			runtime_row_parsers::program_issue_mapping_row,
		)?;
		let mut records = Vec::new();

		for row in rows {
			records.push(row?);
		}

		Ok(records)
	}
}
