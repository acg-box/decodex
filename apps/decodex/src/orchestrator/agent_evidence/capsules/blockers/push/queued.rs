use std::path::Path;

use crate::orchestrator::agent_evidence::{
	self, AgentBlocker, AgentEvidenceProjectView, AgentRunCapsuleRef,
};

pub(crate) fn push_queued_candidate_blockers(
	blockers: &mut Vec<AgentBlocker>,
	project_view: &AgentEvidenceProjectView<'_>,
	blockers_dir: &Path,
	run_refs: &[AgentRunCapsuleRef],
) {
	for candidate in &project_view.queued_candidates {
		if candidate.classification != "blocked" && candidate.attention.is_none() {
			continue;
		}

		let issue_key =
			agent_evidence::issue_key(Some(&candidate.issue_identifier), &candidate.issue_id);
		let reason_code = candidate
			.attention
			.as_ref()
			.and_then(|attention| attention.attention_error_class.as_deref())
			.unwrap_or(candidate.reason.as_str());

		blockers.push(AgentBlocker {
			evidence_ref: agent_evidence::blocker_evidence_ref(
				project_view.project_id,
				&issue_key,
				reason_code,
			),
			project_id: project_view.project_id.to_owned(),
			surface: String::from("intake_queue"),
			issue_id: Some(candidate.issue_id.clone()),
			issue_identifier: Some(candidate.issue_identifier.clone()),
			run_id: candidate.attention.as_ref().and_then(|attention| attention.run_id.clone()),
			attempt_number: candidate
				.attention
				.as_ref()
				.and_then(|attention| attention.attempt_number),
			classification: candidate.classification.clone(),
			reason_code: reason_code.to_owned(),
			reason: candidate
				.attention
				.as_ref()
				.map(|attention| attention.summary.clone())
				.unwrap_or_else(|| candidate.reason.clone()),
			next_action: candidate
				.attention
				.as_ref()
				.and_then(|attention| attention.attention_next_action.clone())
				.unwrap_or_else(|| {
					String::from(
						"Inspect the queued candidate and retained worktree before retrying.",
					)
				}),
			blocker_snapshot_path: agent_evidence::blocker_snapshot_path(blockers_dir, &issue_key)
				.display()
				.to_string(),
			related_run_capsule_path: candidate
				.attention
				.as_ref()
				.and_then(|attention| attention.run_id.as_deref())
				.and_then(|run_id| run_refs.iter().find(|run_ref| run_ref.run_id == run_id))
				.map(|run_ref| run_ref.path.clone()),
		});
	}
}
