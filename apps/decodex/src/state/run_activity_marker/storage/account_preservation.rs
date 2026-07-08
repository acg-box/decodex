use crate::state::{
	CodexAccountActivitySummary, run_activity_marker::record::RunActivityMarkerRecord,
};

pub(crate) fn preserve_current_run_account_marker_fields(
	current: &RunActivityMarkerRecord,
	next: &mut RunActivityMarkerRecord,
) {
	if current.run_id != next.run_id || current.attempt_number != next.attempt_number {
		return;
	}

	let Some(current_account) = selected_marker_account(current).cloned() else {
		return;
	};
	let keep_current_account = match next.account.as_ref() {
		Some(next_account) =>
			account_marker_observed_unix_epoch(&current_account)
				> account_marker_observed_unix_epoch(next_account),
		None => true,
	};

	if keep_current_account {
		next.account = Some(current_account.clone());
		next.accounts = if current.accounts.is_empty() {
			vec![current_account]
		} else {
			current.accounts.clone()
		};
	} else if next.accounts.is_empty() && !current.accounts.is_empty() {
		next.accounts = current.accounts.clone();
	}
}

fn selected_marker_account(
	marker: &RunActivityMarkerRecord,
) -> Option<&CodexAccountActivitySummary> {
	marker
		.account
		.as_ref()
		.or_else(|| {
			marker.accounts.iter().find(|account| account.status.eq_ignore_ascii_case("selected"))
		})
		.or_else(|| marker.accounts.first())
}

fn account_marker_observed_unix_epoch(account: &CodexAccountActivitySummary) -> i64 {
	[account.selected_at_unix_epoch, account.checked_at_unix_epoch]
		.into_iter()
		.flatten()
		.max()
		.unwrap_or(0)
}
