#[derive(Debug)]
pub(in crate::agent::tracker_tool_bridge::tools::manual_attention) struct NormalizedManualAttentionComment
{
	pub(super) error_class: String,
	pub(super) next_action: String,
	pub(super) blockers: Vec<String>,
	pub(super) evidence: Vec<String>,
	pub(super) failed_command: Option<String>,
	pub(super) raw_error: Option<String>,
	pub(super) summary: Option<String>,
	pub(super) decision_request: Option<NormalizedAuthorityDecisionRequest>,
}

#[derive(Debug)]
pub(in crate::agent::tracker_tool_bridge::tools::manual_attention) struct NormalizedAuthorityDecisionRequest
{
	pub(super) boundary_check_id: i64,
	pub(super) decision_request_id: String,
	pub(super) reason_code: String,
	pub(super) boundary_type: String,
	pub(super) proposed_change: String,
	pub(super) why_exceeds_authority: String,
	pub(super) options: Vec<NormalizedAuthorityDecisionOption>,
	pub(super) recommendation: String,
	pub(super) resume_condition: String,
	pub(super) retained_worktree_evidence: Vec<String>,
	pub(super) retained_diff_evidence: Vec<String>,
	pub(super) recovery_attempt_context: Vec<String>,
}

#[derive(Debug)]
pub(in crate::agent::tracker_tool_bridge::tools::manual_attention) struct NormalizedAuthorityDecisionOption
{
	pub(super) label: String,
	pub(super) description: String,
}
