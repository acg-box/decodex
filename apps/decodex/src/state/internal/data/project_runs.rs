use crate::state::{
	internal::data::StateData,
	models::ProjectRunStatus,
	runtime_records::{RunAttemptRecord, RunControlChannelRecord},
};

impl StateData {
	pub(in crate::state) fn project_run_status(
		&self,
		project_id: &str,
		attempt: &RunAttemptRecord,
	) -> Option<ProjectRunStatus> {
		let worktree = self.worktrees.get(&attempt.issue_id);
		let run_lease = self
			.leases
			.get(&attempt.issue_id)
			.is_some_and(|lease| lease.project_id == project_id && lease.run_id == attempt.run_id);
		let remembered_project = attempt.project_id.as_deref() == Some(project_id);
		let in_project = remembered_project
			|| worktree.is_some_and(|mapping| mapping.project_id == project_id)
			|| run_lease;

		if !in_project {
			return None;
		}

		let event_summary = self.protocol_event_summary(&attempt.run_id);
		let run_activity_summary = self.run_activity_summaries.get(&attempt.run_id);
		let control_channel = self
			.control_channels
			.get(&attempt.run_id)
			.filter(|channel| {
				channel.project_id == project_id
					&& channel.issue_id == attempt.issue_id
					&& channel.attempt_number == attempt.attempt_number
			})
			.map(RunControlChannelRecord::as_public);
		let mut recovery_evidence = vec![String::from("run_attempt")];

		if run_lease {
			recovery_evidence.push(String::from("active_lease"));
		}
		if control_channel.is_some() {
			recovery_evidence.push(String::from("run_control_channel"));
		}
		if event_summary.event_count > 0 {
			recovery_evidence.push(format!("protocol_events:{}", event_summary.event_count));
		}
		if run_activity_summary.and_then(|summary| summary.child_agent_activity.as_ref()).is_some()
		{
			recovery_evidence.push(String::from("child_agent_activity_summary"));
		}
		if run_activity_summary.and_then(|summary| summary.protocol_activity.as_ref()).is_some() {
			recovery_evidence.push(String::from("protocol_activity_summary"));
		}

		Some(ProjectRunStatus {
			run_id: attempt.run_id.clone(),
			issue_id: attempt.issue_id.clone(),
			attempt_number: attempt.attempt_number,
			status: attempt.status.clone(),
			thread_id: attempt.thread_id.clone(),
			turn_id: attempt.turn_id.clone(),
			updated_at: attempt.updated_at.clone(),
			updated_at_unix: attempt.updated_at_unix,
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
			recovery_source: String::from("recorded"),
			recovery_evidence,
			recovery_gaps: Vec::new(),
		})
	}

	pub(in crate::state) fn project_id_for_run(
		&self,
		issue_id: &str,
		run_id: &str,
	) -> Option<String> {
		self.leases
			.get(issue_id)
			.filter(|lease| lease.run_id == run_id)
			.map(|lease| lease.project_id.clone())
			.or_else(|| self.worktrees.get(issue_id).map(|mapping| mapping.project_id.clone()))
	}

	pub(in crate::state) fn remember_run_project(
		&mut self,
		project_id: &str,
		issue_id: &str,
		run_id: Option<&str>,
	) {
		for attempt in self
			.run_attempts
			.values_mut()
			.filter(|attempt| attempt.issue_id == issue_id)
			.filter(|attempt| run_id.is_none_or(|run_id| attempt.run_id == run_id))
		{
			attempt.project_id = Some(project_id.to_owned());
		}
	}
}
