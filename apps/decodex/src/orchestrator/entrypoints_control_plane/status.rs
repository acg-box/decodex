use super::super::{
	OperatorCodexAccountControlStatus, OperatorProjectStatus, OperatorStatusSnapshot,
	ProjectRegistration, operator_github_cli_authority_from_registration,
};

pub(crate) fn empty_control_plane_snapshot(limit: usize) -> OperatorStatusSnapshot {
	OperatorStatusSnapshot {
		project_id: String::from("all"),
		run_limit: limit,
		status_source: None,
		snapshot_age_seconds: None,
		warnings: Vec::new(),
		warning_details: Vec::new(),
		connector_backoffs: Vec::new(),
		projects: Vec::new(),
		account_control: OperatorCodexAccountControlStatus {
			mode: String::from("balanced"),
			account_selector: None,
		},
		accounts: Vec::new(),
		current_lanes: Vec::new(),
		recent_runs: Vec::new(),
		history_lanes: Vec::new(),
		execution_programs: Vec::new(),
		queued_candidates: Vec::new(),
		worktrees: Vec::new(),
		post_review_lanes: Vec::new(),
	}
}

pub(super) fn operator_project_status_from_registration(
	project: &ProjectRegistration,
	warning_count: usize,
) -> OperatorProjectStatus {
	OperatorProjectStatus {
		project_id: project.service_id().to_owned(),
		config_path: project.config_path().display().to_string(),
		repo_root: project.repo_root().display().to_string(),
		enabled: project.enabled(),
		github_cli_authority: operator_github_cli_authority_from_registration(project),
		current_lane_count: 0,
		running_lane_count: 0,
		queued_candidate_count: 0,
		post_review_lane_count: 0,
		retained_worktree_count: 0,
		waiting_lane_count: 0,
		attention_count: 0,
		cleanup_blocked_count: 0,
		cleanup_pending_count: 0,
		connector_state: if project.enabled() {
			if warning_count == 0 { String::from("ok") } else { String::from("degraded") }
		} else {
			String::from("disabled")
		},
		last_activity_at: None,
		warning_count,
	}
}

pub(super) fn operator_project_status_from_dev_registration(
	project: &ProjectRegistration,
) -> OperatorProjectStatus {
	OperatorProjectStatus {
		project_id: project.service_id().to_owned(),
		config_path: project.config_path().display().to_string(),
		repo_root: project.repo_root().display().to_string(),
		enabled: project.enabled(),
		github_cli_authority: operator_github_cli_authority_from_registration(project),
		current_lane_count: 0,
		running_lane_count: 0,
		queued_candidate_count: 0,
		post_review_lane_count: 0,
		retained_worktree_count: 0,
		waiting_lane_count: 0,
		attention_count: 0,
		cleanup_blocked_count: 0,
		cleanup_pending_count: 0,
		connector_state: if project.enabled() {
			String::from("dev")
		} else {
			String::from("disabled")
		},
		last_activity_at: None,
		warning_count: usize::from(project.enabled()),
	}
}
