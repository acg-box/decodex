use crate::orchestrator::{
	OperatorConnectorBackoffStatus, OperatorWorktreeStatus,
	agent_evidence::{
		AgentBlocker, AgentConnectorBackoff, AgentRecoveryContract, AgentRecoveryWorktree,
	},
};

pub(in crate::orchestrator) fn agent_connector_backoff(
	backoff: &OperatorConnectorBackoffStatus,
) -> AgentConnectorBackoff {
	AgentConnectorBackoff {
		evidence_ref: format!(
			"connector:{}/{}:{}",
			backoff.project_id, backoff.connector, backoff.sync_phase
		),
		connector: backoff.connector.clone(),
		sync_phase: backoff.sync_phase.clone(),
		quota_class: backoff.quota_class.clone(),
		reset_at: backoff.reset_at.clone(),
		reset_unix_epoch: backoff.reset_unix_epoch,
		reset_source: backoff.reset_source.clone(),
		retry_after_seconds: backoff.retry_after_seconds,
		warning: backoff.warning.clone(),
		next_action: backoff.next_action.clone(),
	}
}

pub(in crate::orchestrator) fn agent_recovery_worktree(
	role: &str,
	worktree: &OperatorWorktreeStatus,
) -> AgentRecoveryWorktree {
	AgentRecoveryWorktree {
		issue_id: worktree.issue_id.clone(),
		issue_identifier: worktree.issue_identifier.clone(),
		issue_state: worktree.issue_state.clone(),
		branch_name: worktree.branch_name.clone(),
		worktree_path: worktree.worktree_path.clone(),
		role: role.to_owned(),
		ownership: worktree.ownership.clone(),
		ownership_reason: worktree.ownership_reason.clone(),
		hygiene_classification: worktree
			.hygiene
			.as_ref()
			.map(|hygiene| hygiene.classification.clone()),
		hygiene_reason: worktree.hygiene.as_ref().map(|hygiene| hygiene.reason.clone()),
	}
}

pub(in crate::orchestrator) fn agent_recovery_contract(
	blocker: &AgentBlocker,
) -> Option<AgentRecoveryContract> {
	let command = if blocker.reason_code == "missing_review_handoff_record" {
		blocker
			.issue_identifier
			.as_ref()
			.map(|issue| format!("decodex recover review-handoff diagnose {issue} --json"))
	} else {
		None
	};

	if command.is_none() && blocker.surface != "running_lane" && blocker.surface != "intake_queue" {
		return None;
	}

	Some(AgentRecoveryContract {
		evidence_ref: blocker.evidence_ref.clone(),
		kind: blocker.surface.clone(),
		issue_identifier: blocker.issue_identifier.clone(),
		reason_code: blocker.reason_code.clone(),
		command,
		next_action: blocker.next_action.clone(),
	})
}
