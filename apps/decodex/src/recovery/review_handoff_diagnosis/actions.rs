use crate::tracker;

pub(super) fn missing_handoff_next_action(service_id: &str, issue_identifier: &str) -> String {
	format!(
		"Inspect PR lineage and ensure label `{}` is present. Use `decodex recover review-handoff rebind {} --pr <URL>` for a retained lane PR, or `decodex recover review-handoff adopt {} --pr <URL>` from the managed worktree for a human-owned PR takeover.",
		tracker::automation_active_label(service_id),
		issue_identifier,
		issue_identifier
	)
}

pub(super) fn bound_handoff_next_action(
	service_id: &str,
	issue_identifier: &str,
	pr_url: &str,
	active_label_present: Option<bool>,
) -> String {
	if active_label_present == Some(false) {
		return format!(
			"Run `decodex recover review-handoff rebind {issue_identifier} --pr {pr_url} --dry-run`, then rerun without `--dry-run` to restore `{}` ownership if validation passes.",
			tracker::automation_active_label(service_id),
		);
	}

	String::from("Continue the existing post-review lifecycle; no rebind is needed.")
}

pub(super) fn inspect_handoff_next_action(issue_identifier: &str, pr_url: &str) -> String {
	format!(
		"Inspect the retained worktree and PR `{pr_url}`; run `decodex recover review-handoff rebind {issue_identifier} --pr {pr_url}` only after the mismatch is repaired."
	)
}

pub(super) fn rebind_refresh_next_action(issue_identifier: &str, pr_url: &str) -> String {
	format!(
		"Run `decodex recover review-handoff rebind {issue_identifier} --pr {pr_url} --dry-run`, then rerun without `--dry-run` to refresh the retained lifecycle record if validation passes."
	)
}

pub(super) fn rebind_state_transition_next_action(issue_identifier: &str, pr_url: &str) -> String {
	format!(
		"Run `decodex recover review-handoff rebind {issue_identifier} --pr {pr_url} --dry-run`, then rerun without `--dry-run` to complete the pending issue-state transition if validation passes."
	)
}

pub(super) fn issue_state_mismatch_next_action(
	success_state: &str,
	in_progress_state: &str,
) -> String {
	format!(
		"Move the issue to `{success_state}` or `{in_progress_state}` only after confirming the retained handoff lineage still belongs to the current lane."
	)
}
