use crate::orchestrator::{self, OperatorGitHubCliAuthority, OperatorStatusSnapshot};

pub(super) fn append_rendered_attention_summary(
	output: &mut String,
	snapshot: &OperatorStatusSnapshot,
) {
	let current_attention_count = snapshot
		.projects
		.iter()
		.find(|project| project.project_id == snapshot.project_id)
		.or_else(|| snapshot.projects.first())
		.map_or_else(
			|| orchestrator::project_attention_count(snapshot, None),
			|project| project.attention_count,
		);
	let history_only_attention_count = orchestrator::project_history_only_attention_count(snapshot);

	output.push_str(&format!("Current attention: {current_attention_count}\n"));
	output.push_str(&format!("History-only terminal attention: {history_only_attention_count}\n"));

	if current_attention_count == 0 && history_only_attention_count > 0 {
		output.push_str(
			"Current attention action: none; terminal attention rows below are Run Ledger history only.\n",
		);
	}
}

pub(super) fn append_rendered_github_cli_authority(
	output: &mut String,
	snapshot: &OperatorStatusSnapshot,
) {
	if let Some(authority) = rendered_project_github_cli_authority(snapshot) {
		output.push_str(&format!(
			"GitHub CLI: tier={} available={} command_path={} resolved_path={} configured_path={} next_action={}\n",
			authority.discovery_tier,
			authority.available,
			authority.command_path,
			authority.resolved_path.as_deref().unwrap_or("none"),
			authority.configured_path.as_deref().unwrap_or("none"),
			authority.next_action
		));
	}
}

fn rendered_project_github_cli_authority(
	snapshot: &OperatorStatusSnapshot,
) -> Option<&OperatorGitHubCliAuthority> {
	snapshot
		.projects
		.iter()
		.find(|project| project.project_id == snapshot.project_id)
		.or_else(|| snapshot.projects.first())
		.map(|project| &project.github_cli_authority)
}
