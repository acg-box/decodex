use crate::state::{
	ProjectRunStatus, RunControlChannelRecord, StateData,
	project_run_recovery::candidate::ProjectRunRecoveryCandidate,
};

pub(in crate::state) fn project_run_status_from_recovery_candidate(
	state: &StateData,
	candidate: &ProjectRunRecoveryCandidate,
) -> Option<ProjectRunStatus> {
	if state.run_attempts.contains_key(&candidate.run_id) {
		return None;
	}

	let worktree = state.worktrees.get(&candidate.issue_id);
	let run_lease = state.leases.get(&candidate.issue_id).is_some_and(|lease| {
		lease.project_id == candidate.project_id && lease.run_id == candidate.run_id
	});
	let event_summary = state.protocol_event_summary(&candidate.run_id);
	let run_activity_summary = state.run_activity_summaries.get(&candidate.run_id);
	let control_channel = state
		.control_channels
		.get(&candidate.run_id)
		.filter(|channel| {
			channel.project_id == candidate.project_id
				&& channel.issue_id == candidate.issue_id
				&& channel.attempt_number == candidate.attempt_number
		})
		.map(RunControlChannelRecord::as_public);

	Some(ProjectRunStatus {
		run_id: candidate.run_id.clone(),
		issue_id: candidate.issue_id.clone(),
		attempt_number: candidate.attempt_number,
		status: candidate.status.clone(),
		thread_id: candidate.thread_id.clone(),
		turn_id: candidate.turn_id.clone(),
		updated_at: candidate.updated_at.clone(),
		updated_at_unix: candidate.updated_at_unix,
		branch_name: worktree.map(|mapping| mapping.branch_name.clone()),
		worktree_path: worktree.map(|mapping| mapping.worktree_path.clone()),
		run_lease,
		event_count: event_summary.event_count,
		last_event_type: event_summary.last_event_type,
		last_event_at: event_summary.last_event_at,
		last_event_at_unix: event_summary.last_event_at_unix,
		control_channel,
		child_agent_activity: run_activity_summary
			.and_then(|summary| summary.child_agent_activity.clone()),
		protocol_activity: run_activity_summary
			.and_then(|summary| summary.protocol_activity.clone()),
		recovery_source: String::from("recovered"),
		recovery_evidence: candidate.evidence.iter().cloned().collect(),
		recovery_gaps: candidate.gaps.iter().cloned().collect(),
	})
}
