use crate::orchestrator::execution_architecture_recovery::{
	ARCHITECTURE_RECOVERY_STARTED_EVENT_TYPE, DecisionContractRecord, IssueRunPlan, Result,
	ServiceConfig, StateStore,
};

pub(in crate::orchestrator::execution_architecture_recovery) fn architecture_recovery_started_count(
	state_store: &StateStore,
	project: &ServiceConfig,
	issue_run: &IssueRunPlan,
) -> Result<usize> {
	Ok(state_store
		.list_private_execution_events_for_issue(project.service_id(), &issue_run.issue.id)?
		.iter()
		.filter(|event| event.event_type() == ARCHITECTURE_RECOVERY_STARTED_EVENT_TYPE)
		.count())
}

pub(in crate::orchestrator::execution_architecture_recovery) fn architecture_recovery_contracts_for_issue(
	state_store: &StateStore,
	project: &ServiceConfig,
	issue_run: &IssueRunPlan,
) -> Result<Vec<DecisionContractRecord>> {
	let mut records = Vec::new();

	for issue_id in [&issue_run.issue.id, &issue_run.issue.identifier] {
		for record in
			state_store.list_decision_contracts_for_issue(project.service_id(), issue_id)?
		{
			if records.iter().all(|existing: &DecisionContractRecord| {
				existing.contract_id() != record.contract_id()
			}) {
				records.push(record);
			}
		}
	}

	records.sort_by(|left, right| left.contract_id().cmp(right.contract_id()));

	Ok(records)
}
