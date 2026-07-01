use crate::state::{
	ProgramIntakePlanRecord, ProgramIssueMappingRecord,
	internal::data::StateData,
	runtime_records::{
		ExecutionProgramRuntimeRecord, ProgramIntakePlanKey, ProgramIssueMappingKey,
	},
};

pub(in crate::state) fn remove_derived_program_intake_state(
	state: &mut StateData,
	project_id: &str,
	program_id: &str,
) {
	state
		.program_intake_plans
		.retain(|key, _record| key.project_id != project_id || key.program_id != program_id);
	state
		.program_issue_mappings
		.retain(|key, _record| key.project_id != project_id || key.program_id != program_id);
}

pub(in crate::state) fn apply_derived_program_intake_state(
	state: &mut StateData,
	record: &ExecutionProgramRuntimeRecord,
) {
	remove_derived_program_intake_state(state, &record.project_id, record.program.program_id());

	for plan in derived_program_intake_plan_records(record) {
		state.program_intake_plans.insert(
			ProgramIntakePlanKey::new(&plan.project_id, &plan.program_id, &plan.plan_id),
			plan,
		);
	}
	for mapping in derived_program_issue_mapping_records(record) {
		state.program_issue_mappings.insert(
			ProgramIssueMappingKey::new(&mapping.project_id, &mapping.program_id, &mapping.node_id),
			mapping,
		);
	}
}

pub(in crate::state) fn derived_program_intake_plan_records(
	record: &ExecutionProgramRuntimeRecord,
) -> Vec<ProgramIntakePlanRecord> {
	record
		.program
		.program_intake_plan()
		.map(|plan| {
			vec![ProgramIntakePlanRecord {
				project_id: record.project_id.clone(),
				program_id: record.program.program_id().to_owned(),
				plan_id: plan.plan_id().to_owned(),
				intake_kind: plan.intake_kind().as_str().to_owned(),
				source_contract_id: plan.source_contract_id().map(str::to_owned),
				accepted_contract_fingerprint: plan.accepted_contract_fingerprint().to_owned(),
				public_summary: plan.public_summary().to_owned(),
				created_at: record.created_at.clone(),
				created_at_unix: record.created_at_unix,
				updated_at: record.updated_at.clone(),
				updated_at_unix: record.updated_at_unix,
			}]
		})
		.unwrap_or_default()
}

pub(in crate::state) fn derived_program_issue_mapping_records(
	record: &ExecutionProgramRuntimeRecord,
) -> Vec<ProgramIssueMappingRecord> {
	record
		.program
		.nodes()
		.iter()
		.filter_map(|node| {
			let issue = node.linear_issue()?;

			Some(ProgramIssueMappingRecord {
				project_id: record.project_id.clone(),
				program_id: record.program.program_id().to_owned(),
				node_id: node.node_id().to_owned(),
				issue_id: issue.issue_id().to_owned(),
				issue_identifier: issue.issue_identifier().to_owned(),
				issue_state: issue.issue_state().to_owned(),
				queue_intent: node.queue_intent().as_str().to_owned(),
				has_active_label: issue.has_active_label(),
				has_opt_out_label: issue.has_opt_out_label(),
				has_needs_attention_label: issue.has_needs_attention_label(),
				has_generic_dispatch_briefing: issue.has_generic_dispatch_briefing(),
				created_at: record.created_at.clone(),
				created_at_unix: record.created_at_unix,
				updated_at: record.updated_at.clone(),
				updated_at_unix: record.updated_at_unix,
			})
		})
		.collect()
}
