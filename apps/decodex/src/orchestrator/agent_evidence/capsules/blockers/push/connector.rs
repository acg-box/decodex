use std::path::Path;

use crate::orchestrator::agent_evidence::{self, AgentBlocker, AgentEvidenceProjectView};

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
