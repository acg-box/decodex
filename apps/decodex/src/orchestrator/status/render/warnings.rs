use crate::orchestrator::{OperatorSnapshotWarningDetail, OperatorStatusSnapshot};

pub(super) fn render_warning_details(snapshot: &OperatorStatusSnapshot) -> String {
	snapshot
		.warnings
		.iter()
		.flat_map(|warning| {
			let details = snapshot
				.warning_details
				.iter()
				.filter(|detail| &detail.warning == warning)
				.collect::<Vec<_>>();

			if details.is_empty() {
				return vec![warning.clone()];
			}

			details.into_iter().map(format_warning_detail).collect()
		})
		.collect::<Vec<_>>()
		.join("; ")
}

fn format_warning_detail(detail: &OperatorSnapshotWarningDetail) -> String {
	let mut parts = vec![detail.warning.clone()];

	if let Some(project_id) = detail.project_id.as_deref() {
		parts.push(format!("project={project_id}"));
	}
	if let Some(repo_root) = detail.repo_root.as_deref() {
		parts.push(format!("repo_root={repo_root}"));
	}

	parts.push(format!("reason={}", detail.reason));

	if let Some(next_action) = detail.next_action.as_deref() {
		parts.push(format!("next_action={next_action}"));
	}

	parts.join(" ")
}
