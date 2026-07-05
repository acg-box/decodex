use std::path::Path;

use crate::orchestrator::agent_evidence::{self, AgentBlocker, AgentEvidenceProjectView};

pub(crate) fn push_recovery_worktree_blockers(
	blockers: &mut Vec<AgentBlocker>,
	project_view: &AgentEvidenceProjectView<'_>,
	blockers_dir: &Path,
) {
	for (role, worktree) in &project_view.recovery_worktrees {
		if worktree.hygiene.is_none() {
			continue;
		}

		let issue_key =
			agent_evidence::issue_key(worktree.issue_identifier.as_deref(), &worktree.issue_id);
		let reason_code = worktree
			.hygiene
			.as_ref()
			.map(|hygiene| hygiene.classification.as_str())
			.unwrap_or(*role);

		blockers.push(AgentBlocker {
			evidence_ref: agent_evidence::blocker_evidence_ref(
				project_view.project_id,
				&issue_key,
				reason_code,
			),
			project_id: project_view.project_id.to_owned(),
			surface: String::from("recovery_worktree"),
			issue_id: Some(worktree.issue_id.clone()),
			issue_identifier: worktree.issue_identifier.clone(),
			run_id: None,
			attempt_number: None,
			classification: (*role).to_owned(),
			reason_code: reason_code.to_owned(),
			reason: worktree.ownership_reason.clone(),
			next_action: String::from("Inspect the retained worktree before cleanup or recovery."),
			blocker_snapshot_path: agent_evidence::blocker_snapshot_path(blockers_dir, &issue_key)
				.display()
				.to_string(),
			related_run_capsule_path: None,
		});
	}
}
