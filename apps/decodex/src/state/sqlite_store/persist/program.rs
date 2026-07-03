use super::{
	Connection, ExecutionProgramRuntimeRecord, Result, StateData, Transaction,
	derived_program_intake_plan_records, derived_program_issue_mapping_records, params,
	sqlite_bool_value,
};

pub(in crate::state::sqlite_store) fn persist_program_intake_state(
	transaction: &Transaction<'_>,
	state: &StateData,
) -> Result<()> {
	for record in state.program_intake_plans.values() {
		transaction.execute(
			"INSERT OR REPLACE INTO program_intake_plans (
					project_id, program_id, plan_id, intake_kind, source_contract_id,
					accepted_contract_fingerprint, public_summary, created_at, created_at_unix,
					updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
			params![
				&record.project_id,
				&record.program_id,
				&record.plan_id,
				&record.intake_kind,
				record.source_contract_id.as_deref(),
				&record.accepted_contract_fingerprint,
				&record.public_summary,
				&record.created_at,
				record.created_at_unix,
				&record.updated_at,
				record.updated_at_unix,
			],
		)?;
	}
	for record in state.program_issue_mappings.values() {
		transaction.execute(
			"INSERT OR REPLACE INTO program_issue_mappings (
					project_id, program_id, node_id, issue_id, issue_identifier, issue_state,
					queue_intent, has_active_label, has_opt_out_label, has_needs_attention_label,
					has_generic_dispatch_briefing, created_at, created_at_unix, updated_at,
					updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
			params![
				&record.project_id,
				&record.program_id,
				&record.node_id,
				&record.issue_id,
				&record.issue_identifier,
				&record.issue_state,
				&record.queue_intent,
				sqlite_bool_value(record.has_active_label),
				sqlite_bool_value(record.has_opt_out_label),
				sqlite_bool_value(record.has_needs_attention_label),
				sqlite_bool_value(record.has_generic_dispatch_briefing),
				&record.created_at,
				record.created_at_unix,
				&record.updated_at,
				record.updated_at_unix,
			],
		)?;
	}

	Ok(())
}

pub(in crate::state::sqlite_store) fn insert_program_intake_state(
	connection: &Connection,
	record: &ExecutionProgramRuntimeRecord,
) -> Result<()> {
	for plan in derived_program_intake_plan_records(record) {
		connection.execute(
			"INSERT OR REPLACE INTO program_intake_plans (
					project_id, program_id, plan_id, intake_kind, source_contract_id,
					accepted_contract_fingerprint, public_summary, created_at, created_at_unix,
					updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
			params![
				&plan.project_id,
				&plan.program_id,
				&plan.plan_id,
				&plan.intake_kind,
				plan.source_contract_id.as_deref(),
				&plan.accepted_contract_fingerprint,
				&plan.public_summary,
				&plan.created_at,
				plan.created_at_unix,
				&plan.updated_at,
				plan.updated_at_unix,
			],
		)?;
	}
	for mapping in derived_program_issue_mapping_records(record) {
		connection.execute(
			"INSERT OR REPLACE INTO program_issue_mappings (
					project_id, program_id, node_id, issue_id, issue_identifier, issue_state,
					queue_intent, has_active_label, has_opt_out_label, has_needs_attention_label,
					has_generic_dispatch_briefing, created_at, created_at_unix, updated_at,
					updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
			params![
				&mapping.project_id,
				&mapping.program_id,
				&mapping.node_id,
				&mapping.issue_id,
				&mapping.issue_identifier,
				&mapping.issue_state,
				&mapping.queue_intent,
				sqlite_bool_value(mapping.has_active_label),
				sqlite_bool_value(mapping.has_opt_out_label),
				sqlite_bool_value(mapping.has_needs_attention_label),
				sqlite_bool_value(mapping.has_generic_dispatch_briefing),
				&mapping.created_at,
				mapping.created_at_unix,
				&mapping.updated_at,
				mapping.updated_at_unix,
			],
		)?;
	}

	Ok(())
}
