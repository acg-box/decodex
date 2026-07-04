use std::path::Path;

use crate::orchestrator::agent_evidence::{
	self, AgentBlocker, AgentEvidenceProjectView, AgentRunCapsuleRef,
	capsules::{blockers::post_review, runs},
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

pub(crate) fn push_warning_blockers(
	blockers: &mut Vec<AgentBlocker>,
	project_view: &AgentEvidenceProjectView<'_>,
	blockers_dir: &Path,
) {
	for warning in &project_view.warnings {
		if warning == "external_observer_status_skipped" {
			continue;
		}

		let issue_key =
			format!("project-{}", agent_evidence::sanitize_evidence_path_component(warning));

		blockers.push(AgentBlocker {
			evidence_ref: agent_evidence::blocker_evidence_ref(
				project_view.project_id,
				&issue_key,
				warning,
			),
			project_id: project_view.project_id.to_owned(),
			surface: String::from("operator_snapshot"),
			issue_id: None,
			issue_identifier: None,
			run_id: None,
			attempt_number: None,
			classification: String::from("snapshot_warning"),
			reason_code: warning.clone(),
			reason: warning.clone(),
			next_action: String::from(
				"Regenerate diagnose output after resolving the unavailable observer or runtime warning.",
			),
			blocker_snapshot_path: agent_evidence::blocker_snapshot_path(blockers_dir, &issue_key)
				.display()
				.to_string(),
			related_run_capsule_path: None,
		});
	}
}

pub(crate) fn push_connector_backoff_blockers(
	blockers: &mut Vec<AgentBlocker>,
	project_view: &AgentEvidenceProjectView<'_>,
	blockers_dir: &Path,
) {
	for backoff in &project_view.connector_backoffs {
		let issue_key = format!(
			"connector-{}",
			agent_evidence::sanitize_evidence_path_component(&backoff.connector)
		);

		blockers.push(AgentBlocker {
			evidence_ref: agent_evidence::blocker_evidence_ref(
				project_view.project_id,
				&issue_key,
				&backoff.warning,
			),
			project_id: project_view.project_id.to_owned(),
			surface: String::from("connector_backoff"),
			issue_id: None,
			issue_identifier: None,
			run_id: None,
			attempt_number: None,
			classification: String::from("backoff"),
			reason_code: backoff.warning.clone(),
			reason: format!("{} {}", backoff.connector, backoff.sync_phase),
			next_action: backoff.next_action.clone(),
			blocker_snapshot_path: agent_evidence::blocker_snapshot_path(blockers_dir, &issue_key)
				.display()
				.to_string(),
			related_run_capsule_path: None,
		});
	}
}
