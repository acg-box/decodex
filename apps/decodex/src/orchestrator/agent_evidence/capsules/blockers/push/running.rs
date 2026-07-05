use std::path::Path;

use crate::orchestrator::agent_evidence::{
	self, AgentBlocker, AgentEvidenceProjectView, AgentRunCapsuleRef, capsules::runs,
};

pub(crate) fn push_run_blockers(
	blockers: &mut Vec<AgentBlocker>,
	project_view: &AgentEvidenceProjectView<'_>,
	blockers_dir: &Path,
	run_refs: &[AgentRunCapsuleRef],
) {
	for run in &project_view.current_lanes {
		if let Some(reason_code) = runs::agent_run_blocker_reason(run) {
			let issue_key =
				agent_evidence::issue_key(run.issue_identifier.as_deref(), &run.issue_id);

			blockers.push(AgentBlocker {
				evidence_ref: agent_evidence::blocker_evidence_ref(
					project_view.project_id,
					&issue_key,
					reason_code,
				),
				project_id: project_view.project_id.to_owned(),
				surface: String::from("running_lane"),
				issue_id: Some(run.issue_id.clone()),
				issue_identifier: run.issue_identifier.clone(),
				run_id: Some(run.run_id.clone()),
				attempt_number: Some(run.attempt_number),
				classification: String::from("attention_required"),
				reason_code: reason_code.to_owned(),
				reason: run.wait_reason.clone().unwrap_or_else(|| reason_code.to_owned()),
				next_action: runs::agent_run_next_action(run)
					.unwrap_or_else(|| String::from("Inspect the run capsule.")),
				blocker_snapshot_path: agent_evidence::blocker_snapshot_path(
					blockers_dir,
					&issue_key,
				)
				.display()
				.to_string(),
				related_run_capsule_path: run_refs
					.iter()
					.find(|run_ref| run_ref.run_id == run.run_id)
					.map(|run_ref| run_ref.path.clone()),
			});
		}
	}
}
