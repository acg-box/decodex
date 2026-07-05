use crate::{
	prelude::Result,
	recovery::{
		self, AdoptValidation, REVIEW_HANDOFF_ADOPT_EVENT, REVIEW_HANDOFF_REBIND_EVENT,
		RebindValidation,
	},
	state::StateStore,
};

pub(in crate::recovery) fn append_review_handoff_rebind_private_event(
	state_store: &StateStore,
	service_id: &str,
	validation: &RebindValidation,
	writeback_stage: &str,
	active_label_restored: bool,
) -> Result<()> {
	state_store
		.append_private_execution_event(
			service_id,
			&validation.issue.id,
			&validation.run_id,
			validation.attempt_number,
			REVIEW_HANDOFF_REBIND_EVENT,
			serde_json::json!({
				"schema": "decodex.review_handoff_recovery_private_event/1",
				"event": REVIEW_HANDOFF_REBIND_EVENT,
				"writeback_stage": writeback_stage,
				"issue_identifier": &validation.issue.identifier,
				"branch": validation.worktree.branch_name(),
				"worktree_path": &validation.worktree_path_for_event,
				"pr_url": recovery::landing_url(&validation.landing_state),
				"pr_head_sha": &validation.local_head_oid,
				"pr_base_ref": &validation.landing_state.base_ref_name,
				"pr_state": &validation.landing_state.state,
				"mergeable": &validation.landing_state.mergeable,
				"merge_state_status": &validation.landing_state.merge_state_status,
				"status_check_rollup_state": &validation.landing_state.status_check_rollup_state,
				"mode": validation.mode.as_str(),
				"active_label_present": validation.active_label_present,
				"active_label_restored": active_label_restored,
				"clear_needs_attention_label": validation.clear_needs_attention_label,
				"next_action": "continue retained post-review lifecycle",
			}),
		)
		.map(|_| ())
}

pub(in crate::recovery) fn append_review_handoff_adopt_private_event(
	state_store: &StateStore,
	service_id: &str,
	validation: &AdoptValidation,
	writeback_stage: &str,
	active_label_restored: bool,
) -> Result<()> {
	state_store
		.append_private_execution_event(
			service_id,
			&validation.issue.id,
			&validation.run_id,
			validation.attempt_number,
			REVIEW_HANDOFF_ADOPT_EVENT,
			serde_json::json!({
				"schema": "decodex.review_handoff_recovery_private_event/1",
				"event": REVIEW_HANDOFF_ADOPT_EVENT,
				"writeback_stage": writeback_stage,
				"issue_identifier": &validation.issue.identifier,
				"branch": &validation.branch_name,
				"worktree_path": &validation.worktree_path_for_event,
				"pr_url": recovery::landing_url(&validation.landing_state),
				"pr_head_sha": &validation.local_head_oid,
				"pr_base_ref": &validation.landing_state.base_ref_name,
				"pr_state": &validation.landing_state.state,
				"mergeable": &validation.landing_state.mergeable,
				"merge_state_status": &validation.landing_state.merge_state_status,
				"status_check_rollup_state": &validation.landing_state.status_check_rollup_state,
				"active_label_present": validation.active_label_present,
				"active_label_restored": active_label_restored,
				"existing_retained_worktree_mapping": validation.previous_worktree_mapping.is_some(),
				"existing_review_handoff_marker": false,
				"manual_takeover_adopt": true,
				"next_action": "continue retained post-review lifecycle",
			}),
		)
		.map(|_| ())
}
