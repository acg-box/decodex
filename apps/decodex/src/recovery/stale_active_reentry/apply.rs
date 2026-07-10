use crate::recovery::stale_active_reentry::{
	local_cleanup, startable_restore,
	types::{
		StaleActiveLocalCleanupReentryInput, StaleActiveReleaseReentryInput,
		StaleActiveStartableStateRestoreReentryInput,
	},
};

pub(in crate::recovery) fn apply_stale_active_release_reentries(
	input: StaleActiveReleaseReentryInput<'_>,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) {
	let local_cleanup_input = StaleActiveLocalCleanupReentryInput {
		run: input.run,
		run_lease: input.run_lease,
		active_shared_claim: input.active_shared_claim,
		queue_label_present: input.labels.queue_label_present,
		active_label_present: input.labels.active_label_present,
		needs_attention_label_present: input.labels.needs_attention_label_present,
		worktree_state: input.worktree_state,
		control_channel: input.control_channel,
	};

	local_cleanup::apply_missing_active_label_retained_cleanup(
		&local_cleanup_input,
		evidence,
		blockers,
	);
	local_cleanup::apply_stale_active_local_cleanup_reentry(
		&local_cleanup_input,
		evidence,
		blockers,
	);
	startable_restore::apply_stale_active_startable_state_restore_reentry(
		StaleActiveStartableStateRestoreReentryInput {
			run: input.run,
			run_lease: input.run_lease,
			active_shared_claim: input.active_shared_claim,
			queue_label_present: input.labels.queue_label_present,
			active_label_present: input.labels.active_label_present,
			needs_attention_label_present: input.labels.needs_attention_label_present,
			issue_state: &input.issue.state.name,
			in_progress_state: input.tracker_policy.in_progress_state(),
			startable_state_id_present: stale_active_startable_state_id_present(&input),
			worktree_state: input.worktree_state,
			control_channel: input.control_channel,
		},
		evidence,
		blockers,
	);
}

fn stale_active_startable_state_id_present(input: &StaleActiveReleaseReentryInput<'_>) -> bool {
	input
		.tracker_policy
		.startable_states()
		.first()
		.and_then(|state_name| input.issue.state_id_for_name(state_name))
		.is_some()
}
