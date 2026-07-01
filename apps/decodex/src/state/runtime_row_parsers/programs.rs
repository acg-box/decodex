use rusqlite::{self, Error, Row};

use crate::{
	execution_program::ExecutionProgram,
	prelude::eyre,
	state::{
		ExecutionProgramRuntimeRecord, ExecutionProgramRuntimeRowParts, ProgramIntakePlanRecord,
		ProgramIssueMappingRecord, runtime_row_parsers::common,
	},
};

pub(in crate::state) fn execution_program_runtime_row_parts(
	row: &Row<'_>,
) -> std::result::Result<ExecutionProgramRuntimeRowParts, Error> {
	Ok(ExecutionProgramRuntimeRowParts {
		project_id: row.get(0)?,
		program_id: row.get(1)?,
		source_contract_id: row.get(2)?,
		payload_json: row.get(3)?,
		created_at: row.get(4)?,
		created_at_unix: row.get(5)?,
		updated_at: row.get(6)?,
		updated_at_unix: row.get(7)?,
	})
}

pub(in crate::state) fn execution_program_record_from_row_parts(
	parts: ExecutionProgramRuntimeRowParts,
) -> crate::prelude::Result<ExecutionProgramRuntimeRecord> {
	let program = serde_json::from_str::<ExecutionProgram>(&parts.payload_json)?;

	program.validate()?;

	if parts.program_id != program.program_id() {
		eyre::bail!(
			"Execution program row `{}` contained payload `{}`.",
			parts.program_id,
			program.program_id()
		);
	}
	if parts.source_contract_id.as_deref() != program.source_contract_id() {
		eyre::bail!(
			"Execution program row `{}` carried source contract `{}` but payload references `{}`.",
			parts.program_id,
			parts.source_contract_id.as_deref().unwrap_or("none"),
			program.source_contract_id().unwrap_or("none")
		);
	}

	Ok(ExecutionProgramRuntimeRecord {
		project_id: parts.project_id,
		source_contract_id: parts.source_contract_id,
		program,
		created_at: parts.created_at,
		created_at_unix: parts.created_at_unix,
		updated_at: parts.updated_at,
		updated_at_unix: parts.updated_at_unix,
	})
}

pub(in crate::state) fn program_intake_plan_row(
	row: &Row<'_>,
) -> std::result::Result<ProgramIntakePlanRecord, Error> {
	Ok(ProgramIntakePlanRecord {
		project_id: row.get(0)?,
		program_id: row.get(1)?,
		plan_id: row.get(2)?,
		intake_kind: row.get(3)?,
		source_contract_id: row.get(4)?,
		accepted_contract_fingerprint: row.get(5)?,
		public_summary: row.get(6)?,
		created_at: row.get(7)?,
		created_at_unix: row.get(8)?,
		updated_at: row.get(9)?,
		updated_at_unix: row.get(10)?,
	})
}

pub(in crate::state) fn program_issue_mapping_row(
	row: &Row<'_>,
) -> std::result::Result<ProgramIssueMappingRecord, Error> {
	Ok(ProgramIssueMappingRecord {
		project_id: row.get(0)?,
		program_id: row.get(1)?,
		node_id: row.get(2)?,
		issue_id: row.get(3)?,
		issue_identifier: row.get(4)?,
		issue_state: row.get(5)?,
		queue_intent: row.get(6)?,
		has_active_label: common::sqlite_bool(row, 7)?,
		has_opt_out_label: common::sqlite_bool(row, 8)?,
		has_needs_attention_label: common::sqlite_bool(row, 9)?,
		has_generic_dispatch_briefing: common::sqlite_bool(row, 10)?,
		created_at: row.get(11)?,
		created_at_unix: row.get(12)?,
		updated_at: row.get(13)?,
		updated_at_unix: row.get(14)?,
	})
}
