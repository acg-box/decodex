use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(crate) struct ScopeArgs {
	pub(crate) issue_id: Option<String>,

	pub(crate) issue_identifier: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TransitionArgs {
	#[serde(flatten)]
	pub(crate) scope: ScopeArgs,
	pub(crate) state: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CommentArgs {
	#[serde(flatten)]
	pub(crate) scope: ScopeArgs,
	pub(crate) kind: String,
	pub(crate) error_class: Option<String>,
	pub(crate) next_action: Option<String>,
	#[serde(default)]
	pub(crate) blockers: Vec<String>,
	#[serde(default)]
	pub(crate) evidence: Vec<String>,
	pub(crate) failed_command: Option<String>,
	pub(crate) raw_error: Option<String>,
	pub(crate) summary: Option<String>,
	pub(crate) decision_request: Option<AuthorityDecisionRequestArgs>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AuthorityDecisionRequestArgs {
	pub(crate) boundary_check_id: i64,
	pub(crate) decision_request_id: String,
	pub(crate) reason_code: String,
	pub(crate) boundary_type: String,
	pub(crate) proposed_change: String,
	pub(crate) why_exceeds_authority: String,
	#[serde(default)]
	pub(crate) options: Vec<AuthorityDecisionOptionArgs>,
	pub(crate) recommendation: String,
	pub(crate) resume_condition: String,
	#[serde(default)]
	pub(crate) retained_worktree_evidence: Vec<String>,
	#[serde(default)]
	pub(crate) retained_diff_evidence: Vec<String>,
	#[serde(default)]
	pub(crate) recovery_attempt_context: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AuthorityDecisionOptionArgs {
	pub(crate) label: String,
	pub(crate) description: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ReviewHandoffArgs {
	#[serde(flatten)]
	pub(crate) scope: ScopeArgs,
	pub(crate) pr_url: String,
	pub(crate) summary: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProgressCheckpointArgs {
	#[serde(flatten)]
	pub(crate) scope: ScopeArgs,
	pub(crate) phase: String,
	pub(crate) openwiki_impact: String,
	pub(crate) focus: String,
	pub(crate) next_action: String,
	#[serde(default)]
	pub(crate) blockers: Vec<String>,
	#[serde(default)]
	pub(crate) evidence: Vec<String>,
	#[serde(default)]
	pub(crate) verification: Vec<String>,
	pub(crate) head_sha: Option<String>,
	pub(crate) branch: Option<String>,
	pub(crate) pr_url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LabelArgs {
	#[serde(flatten)]
	pub(crate) scope: ScopeArgs,
	pub(crate) label: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct TerminalFinalizeArgs {
	#[serde(flatten)]
	pub(crate) scope: ScopeArgs,
	pub(crate) path: String,
}
