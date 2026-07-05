use crate::{
	prelude::Result,
	state::{
		PrivateExecutionEvent, StateStore,
		store_run_control::{types::RunControlAuditTarget, validation},
	},
};

pub(in crate::state::store_run_control::audit) fn append_run_control_audit_event(
	store: &StateStore,
	target: &RunControlAuditTarget,
	outcome: &str,
	reason: &str,
	parent_record_id: Option<i64>,
) -> Result<PrivateExecutionEvent> {
	validation::validate_run_control_action_outcome(outcome)?;

	let channel = target.channel.as_ref();
	let failure_class =
		validation::run_control_action_failure_class(&target.action, outcome, reason);
	let payload = serde_json::json!({
		"schema": "decodex.run_control_action/v1",
		"action": target.action,
		"source": target.source,
		"outcome": outcome,
		"reason": reason,
		"failure_class": failure_class,
		"parent_record_id": parent_record_id,
		"requested": {
			"project_id": target.project_id,
			"issue_id": target.issue_id,
			"run_id": target.run_id,
			"attempt_number": target.attempt_number,
			"thread_id": target.thread_id,
			"turn_id": target.turn_id,
			"timeout_ms": target.timeout_ms,
		},
		"observed": {
			"thread_id": target.current_thread_id.as_deref(),
			"turn_id": target.current_turn_id.as_deref(),
		},
		"lane": {
			"attempt_status": target.attempt_status.as_deref(),
			"run_lease": target.run_lease,
			"branch": target.branch_name.as_deref(),
			"worktree_path": target.worktree_path.as_ref().map(|path| path.display().to_string()),
			"event_count": target.event_count,
			"last_event_type": target.last_event_type.as_deref(),
			"last_event_at": target.last_event_at.as_deref(),
		},
		"metadata": target.metadata.as_ref(),
		"context": target.context.as_ref(),
		"channel": channel.map(|channel| serde_json::json!({
			"transport": channel.transport(),
			"channel_path": channel.channel_path().display().to_string(),
			"status": channel.status(),
			"published_at": channel.published_at(),
			"updated_at": channel.updated_at(),
			"path_exists": channel.channel_path().exists(),
		})),
	});

	store.append_private_execution_event(
		&target.project_id,
		&target.issue_id,
		&target.run_id,
		target.attempt_number,
		"control_action",
		payload,
	)
}
