use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(in crate::agent::tracker_tool_bridge) struct ScopeArgs {
	pub(in crate::agent::tracker_tool_bridge) issue_id: Option<String>,

	pub(in crate::agent::tracker_tool_bridge) issue_identifier: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::agent::tracker_tool_bridge) struct TransitionArgs {
	#[serde(flatten)]
	pub(in crate::agent::tracker_tool_bridge) scope: ScopeArgs,
	pub(in crate::agent::tracker_tool_bridge) state: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::agent::tracker_tool_bridge) struct CommentArgs {
	#[serde(flatten)]
	pub(in crate::agent::tracker_tool_bridge) scope: ScopeArgs,
	pub(in crate::agent::tracker_tool_bridge) kind: String,
	pub(in crate::agent::tracker_tool_bridge) error_class: Option<String>,
	pub(in crate::agent::tracker_tool_bridge) next_action: Option<String>,
	#[serde(default)]
	pub(in crate::agent::tracker_tool_bridge) blockers: Vec<String>,
	#[serde(default)]
	pub(in crate::agent::tracker_tool_bridge) evidence: Vec<String>,
	pub(in crate::agent::tracker_tool_bridge) failed_command: Option<String>,
	pub(in crate::agent::tracker_tool_bridge) raw_error: Option<String>,
	pub(in crate::agent::tracker_tool_bridge) summary: Option<String>,
	pub(in crate::agent::tracker_tool_bridge) decision_request:
		Option<AuthorityDecisionRequestArgs>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::agent::tracker_tool_bridge) struct AuthorityDecisionRequestArgs {
	pub(in crate::agent::tracker_tool_bridge) boundary_check_id: i64,
	pub(in crate::agent::tracker_tool_bridge) decision_request_id: String,
	pub(in crate::agent::tracker_tool_bridge) reason_code: String,
	pub(in crate::agent::tracker_tool_bridge) boundary_type: String,
	pub(in crate::agent::tracker_tool_bridge) proposed_change: String,
	pub(in crate::agent::tracker_tool_bridge) why_exceeds_authority: String,
	#[serde(default)]
	pub(in crate::agent::tracker_tool_bridge) options: Vec<AuthorityDecisionOptionArgs>,
	pub(in crate::agent::tracker_tool_bridge) recommendation: String,
	pub(in crate::agent::tracker_tool_bridge) resume_condition: String,
	#[serde(default)]
	pub(in crate::agent::tracker_tool_bridge) retained_worktree_evidence: Vec<String>,
	#[serde(default)]
	pub(in crate::agent::tracker_tool_bridge) retained_diff_evidence: Vec<String>,
	#[serde(default)]
	pub(in crate::agent::tracker_tool_bridge) recovery_attempt_context: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::agent::tracker_tool_bridge) struct AuthorityDecisionOptionArgs {
	pub(in crate::agent::tracker_tool_bridge) label: String,
	pub(in crate::agent::tracker_tool_bridge) description: String,
}

#[derive(Debug, Deserialize)]
pub(in crate::agent::tracker_tool_bridge) struct ReviewHandoffArgs {
	#[serde(flatten)]
	pub(in crate::agent::tracker_tool_bridge) scope: ScopeArgs,
	pub(in crate::agent::tracker_tool_bridge) pr_url: String,
	pub(in crate::agent::tracker_tool_bridge) summary: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::agent::tracker_tool_bridge) struct ProgressCheckpointArgs {
	#[serde(flatten)]
	pub(in crate::agent::tracker_tool_bridge) scope: ScopeArgs,
	pub(in crate::agent::tracker_tool_bridge) phase: String,
	pub(in crate::agent::tracker_tool_bridge) docs_impact: String,
	pub(in crate::agent::tracker_tool_bridge) focus: String,
	pub(in crate::agent::tracker_tool_bridge) next_action: String,
	#[serde(default)]
	pub(in crate::agent::tracker_tool_bridge) blockers: Vec<String>,
	#[serde(default)]
	pub(in crate::agent::tracker_tool_bridge) evidence: Vec<String>,
	#[serde(default)]
	pub(in crate::agent::tracker_tool_bridge) verification: Vec<String>,
	pub(in crate::agent::tracker_tool_bridge) head_sha: Option<String>,
	pub(in crate::agent::tracker_tool_bridge) branch: Option<String>,
	pub(in crate::agent::tracker_tool_bridge) pr_url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::agent::tracker_tool_bridge) struct LabelArgs {
	#[serde(flatten)]
	pub(in crate::agent::tracker_tool_bridge) scope: ScopeArgs,
	pub(in crate::agent::tracker_tool_bridge) label: String,
}

#[derive(Debug, Deserialize)]
pub(in crate::agent::tracker_tool_bridge) struct TerminalFinalizeArgs {
	#[serde(flatten)]
	pub(in crate::agent::tracker_tool_bridge) scope: ScopeArgs,
	pub(in crate::agent::tracker_tool_bridge) path: String,
}
