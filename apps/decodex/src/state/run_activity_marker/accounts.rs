use crate::state::{
	CodexAccountActivitySummary, run_activity_marker::record::RunActivityMarkerRecord,
};

pub(in crate::state::run_activity_marker) fn normalize_accounts(
	selected: &CodexAccountActivitySummary,
	accounts: &[CodexAccountActivitySummary],
) -> Vec<CodexAccountActivitySummary> {
	let mut normalized =
		if accounts.is_empty() { vec![selected.clone()] } else { accounts.to_vec() };

	if !normalized.iter().any(|account| account.account_fingerprint == selected.account_fingerprint)
	{
		normalized.insert(0, selected.clone());
	}

	normalized
}

pub(in crate::state::run_activity_marker) fn accounts_from_marker_record(
	marker: &RunActivityMarkerRecord,
) -> Vec<CodexAccountActivitySummary> {
	if marker.accounts.is_empty() {
		marker.account.iter().cloned().collect()
	} else {
		marker.accounts.clone()
	}
}
