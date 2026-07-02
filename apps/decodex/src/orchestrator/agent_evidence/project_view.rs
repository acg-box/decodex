use std::collections::BTreeSet;

use crate::orchestrator::{
	self, OperatorConnectorBackoffStatus, OperatorHistoryLaneStatus, OperatorPostReviewLaneStatus,
	OperatorProjectStatus, OperatorQueuedIssueStatus, OperatorRunStatus, OperatorStatusSnapshot,
	OperatorWorktreeStatus,
};

pub(in crate::orchestrator) struct AgentEvidenceProjectView<'a> {
	pub(in crate::orchestrator) project_id: &'a str,
	pub(in crate::orchestrator) warnings: Vec<String>,
	pub(in crate::orchestrator) projects: Vec<&'a OperatorProjectStatus>,
	pub(in crate::orchestrator) connector_backoffs: Vec<&'a OperatorConnectorBackoffStatus>,
	pub(in crate::orchestrator) current_lanes: Vec<&'a OperatorRunStatus>,
	pub(in crate::orchestrator) recent_runs: Vec<&'a OperatorRunStatus>,
	pub(in crate::orchestrator) history_lanes: Vec<&'a OperatorHistoryLaneStatus>,
	pub(in crate::orchestrator) queued_candidates: Vec<&'a OperatorQueuedIssueStatus>,
	pub(in crate::orchestrator) recovery_worktrees: Vec<(&'a str, &'a OperatorWorktreeStatus)>,
	pub(in crate::orchestrator) post_review_lanes: Vec<&'a OperatorPostReviewLaneStatus>,
}
impl<'a> AgentEvidenceProjectView<'a> {
	pub(in crate::orchestrator) fn from_snapshot(
		snapshot: &'a OperatorStatusSnapshot,
		project_id: &'a str,
	) -> Self {
		let single_project_snapshot = snapshot.project_id == project_id;
		let projects = snapshot
			.projects
			.iter()
			.filter(|project| project.project_id == project_id)
			.collect::<Vec<_>>();
		let connector_backoffs = snapshot
			.connector_backoffs
			.iter()
			.filter(|backoff| backoff.project_id == project_id)
			.collect::<Vec<_>>();
		let current_lanes = snapshot
			.current_lanes
			.iter()
			.filter(|run| run.project_id == project_id)
			.collect::<Vec<_>>();
		let recent_runs = snapshot
			.recent_runs
			.iter()
			.filter(|run| run.project_id == project_id)
			.collect::<Vec<_>>();
		let history_lanes = snapshot
			.history_lanes
			.iter()
			.filter(|lane| lane.project_id == project_id)
			.collect::<Vec<_>>();
		let post_review_lanes = snapshot
			.post_review_lanes
			.iter()
			.filter(|lane| {
				lane_issue_belongs_to_project(lane.issue_id.as_str(), project_id, snapshot)
			})
			.collect::<Vec<_>>();
		let queued_candidates = snapshot
			.queued_candidates
			.iter()
			.filter(|candidate| {
				lane_issue_belongs_to_project(candidate.issue_id.as_str(), project_id, snapshot)
			})
			.collect::<Vec<_>>();
		let recovery_worktrees = if single_project_snapshot {
			orchestrator::rendered_recovery_worktrees(snapshot)
		} else {
			orchestrator::rendered_recovery_worktrees(snapshot)
				.into_iter()
				.filter(|(_, worktree)| {
					lane_issue_belongs_to_project(worktree.issue_id.as_str(), project_id, snapshot)
				})
				.collect()
		};

		Self {
			project_id,
			warnings: snapshot.warnings.clone(),
			projects,
			connector_backoffs,
			current_lanes,
			recent_runs,
			history_lanes,
			queued_candidates,
			recovery_worktrees,
			post_review_lanes,
		}
	}
}

pub(in crate::orchestrator) fn agent_evidence_project_ids(
	snapshot: &OperatorStatusSnapshot,
) -> Vec<String> {
	let mut project_ids = BTreeSet::new();

	for project in &snapshot.projects {
		project_ids.insert(project.project_id.clone());
	}
	for run in snapshot.current_lanes.iter().chain(snapshot.recent_runs.iter()) {
		project_ids.insert(run.project_id.clone());
	}
	for lane in &snapshot.history_lanes {
		project_ids.insert(lane.project_id.clone());
	}
	for backoff in &snapshot.connector_backoffs {
		project_ids.insert(backoff.project_id.clone());
	}

	if project_ids.is_empty() && snapshot.project_id != "all" {
		project_ids.insert(snapshot.project_id.clone());
	}

	project_ids.into_iter().collect()
}

fn lane_issue_belongs_to_project(
	issue_id: &str,
	project_id: &str,
	snapshot: &OperatorStatusSnapshot,
) -> bool {
	snapshot
		.current_lanes
		.iter()
		.chain(snapshot.recent_runs.iter())
		.any(|run| run.project_id == project_id && run.issue_id == issue_id)
		|| snapshot
			.history_lanes
			.iter()
			.any(|lane| lane.project_id == project_id && lane.issue_id == issue_id)
		|| snapshot.project_id == project_id
}
