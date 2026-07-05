use std::path::Path;

use crate::orchestrator::agent_evidence::{self, AgentBlocker, AgentEvidenceProjectView};

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
