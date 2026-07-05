use std::path::Path;

use crate::orchestrator::agent_evidence::{
	self, AgentBlocker, AgentEvidenceProjectView, capsules::blockers::post_review,
};

pub(crate) fn push_post_review_lane_blockers(
	blockers: &mut Vec<AgentBlocker>,
	project_view: &AgentEvidenceProjectView<'_>,
	blockers_dir: &Path,
) {
	for lane in &project_view.post_review_lanes {
		if !post_review::post_review_lane_requires_attention(lane) {
			continue;
		}

		let issue_key = agent_evidence::issue_key(Some(&lane.issue_identifier), &lane.issue_id);

		blockers.push(AgentBlocker {
			evidence_ref: agent_evidence::blocker_evidence_ref(
				project_view.project_id,
				&issue_key,
				&lane.reason,
			),
			project_id: project_view.project_id.to_owned(),
			surface: String::from("review_landing"),
			issue_id: Some(lane.issue_id.clone()),
			issue_identifier: Some(lane.issue_identifier.clone()),
			run_id: None,
			attempt_number: None,
			classification: lane.classification.clone(),
			reason_code: lane.reason.clone(),
			reason: lane.reason.clone(),
			next_action: post_review::post_review_lane_next_action(lane, project_view.project_id),
			blocker_snapshot_path: agent_evidence::blocker_snapshot_path(blockers_dir, &issue_key)
				.display()
				.to_string(),
			related_run_capsule_path: None,
		});
	}
}
