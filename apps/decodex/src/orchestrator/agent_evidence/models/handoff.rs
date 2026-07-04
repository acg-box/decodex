use std::path::Path;

use serde::Serialize;

use crate::orchestrator::{
	OperatorGitHubCliAuthority,
	agent_evidence::models::{AgentEvidenceSource, run_capsule::AgentRunCapsuleRef},
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct AgentEvidenceWriteResult {
	pub(crate) project_id: String,
	pub(crate) handoff_index_path: String,
	pub(crate) handoff_index: AgentHandoffIndex,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct AgentEvidenceSummary {
	pub(crate) project_count: usize,
	pub(crate) current_lane_count: usize,
	pub(crate) recent_run_count: usize,
	pub(crate) history_lane_count: usize,
	pub(crate) queued_candidate_count: usize,
	pub(crate) post_review_lane_count: usize,
	pub(crate) recovery_worktree_count: usize,
	pub(crate) blocker_count: usize,
	pub(crate) run_capsule_count: usize,
	pub(crate) connector_backoff_count: usize,
	pub(crate) warning_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct AgentHandoffIndex {
	pub(crate) schema: &'static str,
	pub(crate) project_id: String,
	pub(crate) generated_at: String,
	pub(crate) source: String,
	pub(crate) evidence_root: String,
	pub(crate) handoff_index_path: String,
	pub(crate) blockers_dir: String,
	pub(crate) runs_dir: String,
	pub(crate) events_path: String,
	pub(crate) summary: AgentEvidenceSummary,
	pub(crate) github_cli_authority: Option<OperatorGitHubCliAuthority>,
	pub(crate) warnings: Vec<String>,
	pub(crate) connector_backoffs: Vec<AgentConnectorBackoff>,
	pub(crate) blockers: Vec<AgentBlocker>,
	pub(crate) run_capsules: Vec<AgentRunCapsuleRef>,
	pub(crate) recovery_worktrees: Vec<AgentRecoveryWorktree>,
	pub(crate) recovery_contracts: Vec<AgentRecoveryContract>,
}

pub(crate) struct PrivateEvidenceTarget {
	pub(crate) issue_id: String,
	pub(crate) issue_identifier: Option<String>,
	pub(crate) run_id: String,
	pub(crate) attempt_number: i64,
}

pub(crate) struct AgentEvidenceFileWriteContext<'a> {
	pub(crate) project_id: &'a str,
	pub(crate) generated_at: &'a str,
	pub(crate) source: AgentEvidenceSource,
	pub(crate) handoff_index_path: &'a Path,
	pub(crate) blockers_dir: &'a Path,
	pub(crate) events_path: &'a Path,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct AgentConnectorBackoff {
	pub(crate) evidence_ref: String,
	pub(crate) connector: String,
	pub(crate) sync_phase: String,
	pub(crate) quota_class: String,
	pub(crate) reset_at: String,
	pub(crate) reset_unix_epoch: i64,
	pub(crate) reset_source: String,
	pub(crate) retry_after_seconds: i64,
	pub(crate) warning: String,
	pub(crate) next_action: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct AgentBlocker {
	pub(crate) evidence_ref: String,
	pub(crate) project_id: String,
	pub(crate) surface: String,
	pub(crate) issue_id: Option<String>,
	pub(crate) issue_identifier: Option<String>,
	pub(crate) run_id: Option<String>,
	pub(crate) attempt_number: Option<i64>,
	pub(crate) classification: String,
	pub(crate) reason_code: String,
	pub(crate) reason: String,
	pub(crate) next_action: String,
	pub(crate) blocker_snapshot_path: String,
	pub(crate) related_run_capsule_path: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct AgentBlockerSnapshot {
	pub(crate) schema: &'static str,
	pub(crate) project_id: String,
	pub(crate) generated_at: String,
	pub(crate) issue_id: Option<String>,
	pub(crate) issue_identifier: Option<String>,
	pub(crate) blockers: Vec<AgentBlocker>,
	pub(crate) related_run_capsules: Vec<AgentRunCapsuleRef>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct AgentRecoveryWorktree {
	pub(crate) issue_id: String,
	pub(crate) issue_identifier: Option<String>,
	pub(crate) issue_state: Option<String>,
	pub(crate) branch_name: String,
	pub(crate) worktree_path: String,
	pub(crate) role: String,
	pub(crate) ownership: String,
	pub(crate) ownership_reason: String,
	pub(crate) hygiene_classification: Option<String>,
	pub(crate) hygiene_reason: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct AgentRecoveryContract {
	pub(crate) evidence_ref: String,
	pub(crate) kind: String,
	pub(crate) issue_identifier: Option<String>,
	pub(crate) reason_code: String,
	pub(crate) command: Option<String>,
	pub(crate) next_action: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct AgentEvidenceEvent {
	pub(crate) schema: &'static str,
	pub(crate) project_id: String,
	pub(crate) generated_at: String,
	pub(crate) source: String,
	pub(crate) handoff_index_path: String,
	pub(crate) blocker_count: usize,
	pub(crate) run_capsule_count: usize,
	pub(crate) warning_count: usize,
	pub(crate) connector_backoff_count: usize,
}
