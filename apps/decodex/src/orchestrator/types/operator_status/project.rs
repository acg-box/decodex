use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct OperatorConnectorBackoffStatus {
	pub(crate) project_id: String,
	pub(crate) connector: String,
	pub(crate) sync_phase: String,
	pub(crate) quota_class: String,
	pub(crate) reset_at: String,
	pub(crate) reset_unix_epoch: i64,
	pub(crate) reset_source: String,
	pub(crate) retry_after_seconds: i64,
	pub(crate) next_action: String,
	pub(crate) warning: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct OperatorProjectStatus {
	pub(crate) project_id: String,
	pub(crate) config_path: String,
	pub(crate) repo_root: String,
	pub(crate) enabled: bool,
	pub(crate) github_cli_authority: OperatorGitHubCliAuthority,
	pub(crate) current_lane_count: usize,
	pub(crate) running_lane_count: usize,
	pub(crate) queued_candidate_count: usize,
	pub(crate) post_review_lane_count: usize,
	pub(crate) retained_worktree_count: usize,
	pub(crate) waiting_lane_count: usize,
	pub(crate) attention_count: usize,
	pub(crate) cleanup_blocked_count: usize,
	pub(crate) cleanup_pending_count: usize,
	pub(crate) connector_state: String,
	pub(crate) last_activity_at: Option<String>,
	pub(crate) warning_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct OperatorGitHubCliAuthority {
	pub(crate) command_path: String,
	pub(crate) resolved_path: Option<String>,
	pub(crate) configured_path: Option<String>,
	pub(crate) discovery_tier: String,
	pub(crate) available: bool,
	pub(crate) next_action: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct OperatorCodexAccountControlStatus {
	pub(crate) mode: String,
	pub(crate) account_selector: Option<String>,
}
