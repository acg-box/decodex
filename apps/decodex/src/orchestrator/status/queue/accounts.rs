use crate::{
	config::ServiceConfig,
	orchestrator::status::{AccountActivityMode, CodexAccountActivitySummary, CodexAccountPool},
};

pub(crate) fn codex_account_activity_summaries(
	project: &ServiceConfig,
	warnings: &mut Vec<String>,
	mode: AccountActivityMode,
) -> Vec<CodexAccountActivitySummary> {
	let Some(accounts_config) = project.codex().accounts() else {
		return Vec::new();
	};
	let accounts = CodexAccountPool::from_config(accounts_config).and_then(|pool| match mode {
		AccountActivityMode::Probe => pool.account_activity_summaries_cached(false),
		AccountActivityMode::Snapshot => pool.account_activity_summaries_snapshot(),
	});

	match accounts {
		Ok(accounts) => accounts,
		Err(error) => {
			tracing::warn!(
				project_id = project.service_id(),
				error = %error,
				"Codex accounts snapshot could not be loaded."
			);

			warnings.push(String::from("codex_accounts_unavailable"));

			Vec::new()
		},
	}
}
