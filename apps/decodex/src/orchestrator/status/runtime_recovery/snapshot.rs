use crate::{
	orchestrator::status::{
		CodexAccountActivitySummary, RecoveredRuntimeState, ServiceConfig, StateStore,
	},
	prelude::Result,
};

pub(crate) fn hydrate_status_snapshot_state(
	_project: &ServiceConfig,
	_state_store: &StateStore,
	_recovered_state: RecoveredRuntimeState,
) -> Result<()> {
	Ok(())
}

pub(crate) fn append_primary_account_if_missing(
	accounts: &mut Vec<CodexAccountActivitySummary>,
	account: Option<&CodexAccountActivitySummary>,
) {
	if accounts.is_empty()
		&& let Some(account) = account
	{
		accounts.push(account.clone());
	}
}
