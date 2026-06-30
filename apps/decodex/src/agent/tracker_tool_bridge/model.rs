use std::{
	error::Error,
	fmt::{Display, Formatter},
	path::PathBuf,
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::ReviewLevel;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct DynamicToolSpec {
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) namespace: Option<String>,
	pub(crate) description: String,
	#[serde(rename = "deferLoading", default, skip_serializing_if = "std::ops::Not::not")]
	pub(crate) defer_loading: bool,
	#[serde(rename = "inputSchema")]
	pub(crate) input_schema: Value,
	pub(crate) name: String,
}
impl DynamicToolSpec {
	pub(crate) fn new(
		name: impl Into<String>,
		description: impl Into<String>,
		input_schema: Value,
	) -> Self {
		Self {
			namespace: None,
			description: description.into(),
			defer_loading: false,
			input_schema,
			name: name.into(),
		}
	}

	pub(crate) fn deferred(mut self) -> Self {
		self.defer_loading = true;

		self
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ReviewHandoffContext {
	pub(crate) attempt_number: i64,
	pub(crate) branch_name: String,
	pub(crate) run_id: String,
	pub(crate) service_id: String,
	pub(crate) worktree_path: String,
	pub(crate) cwd: PathBuf,
	pub(crate) github_token_env_var: Option<String>,
	pub(crate) github_command_path: Option<PathBuf>,
	pub(crate) review_level: ReviewLevel,
	pub(crate) mode: ReviewExecutionMode,
	pub(crate) recorded_pr_url: Option<String>,
}
impl ReviewHandoffContext {
	pub(crate) fn decodex_review_checkpoint_enabled(&self) -> bool {
		self.review_level.requires_review_checkpoint()
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ReviewHandoffWritebackFailed {
	pub(crate) issue_identifier: String,
	pub(crate) run_id: String,
	pub(crate) pr_url: String,
	pub(crate) success_state: String,
	pub(crate) source: String,
}
impl Display for ReviewHandoffWritebackFailed {
	fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
		write!(
			f,
			"Run `{}` failed to finalize the review handoff for issue `{}` around target state `{}` and PR `{}`: {}",
			self.run_id, self.issue_identifier, self.success_state, self.pr_url, self.source
		)
	}
}

impl Error for ReviewHandoffWritebackFailed {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PullRequestDetails {
	pub(super) base_ref_name: String,
	pub(super) head_ref_name: String,
	pub(super) head_ref_oid: String,
	pub(super) head_repository_name: String,
	pub(super) head_repository_owner: String,
	pub(super) is_draft: bool,
	pub(super) state: String,
	pub(super) url: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LocalRepoDetails {
	pub(super) default_branch: String,
	pub(super) head_oid: String,
	pub(super) head_tree_oid: String,
	pub(super) repository_name: String,
	pub(super) repository_owner: String,
	pub(super) review_blocking_changes: Vec<String>,
}
impl LocalRepoDetails {
	pub(super) fn review_worktree_clean(&self) -> bool {
		self.review_blocking_changes.is_empty()
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct DynamicToolCallResponse {
	#[serde(rename = "contentItems")]
	pub(crate) content_items: Vec<DynamicToolContentItem>,
	pub(crate) success: bool,
}
impl DynamicToolCallResponse {
	pub(crate) fn success(message: String) -> Self {
		Self { content_items: vec![DynamicToolContentItem::text(message)], success: true }
	}

	pub(crate) fn failure(message: String) -> Self {
		Self { content_items: vec![DynamicToolContentItem::text(message)], success: false }
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ReviewPolicyStopRequested {
	pub(crate) head_sha: String,
	pub(crate) issue_identifier: String,
	pub(crate) fingerprint: Option<String>,
	pub(crate) nonclean_rounds: Option<i64>,
	pub(crate) reason: ReviewPolicyStopReason,
	pub(crate) run_id: String,
}
impl Display for ReviewPolicyStopRequested {
	fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
		match self.reason {
			ReviewPolicyStopReason::Exhausted => write!(
				f,
				"Run `{}` for issue `{}` exhausted the runtime-owned review convergence budget at HEAD `{}` after {} non-clean rounds{}.",
				self.run_id,
				self.issue_identifier,
				self.head_sha,
				self.nonclean_rounds.unwrap_or_default(),
				self.fingerprint.as_ref().map_or_else(String::new, |fingerprint| format!(
					" for finding fingerprint `{fingerprint}`"
				))
			),
			ReviewPolicyStopReason::ArchitectureReviewRequired => write!(
				f,
				"Run `{}` for issue `{}` recorded `needs_architecture_review` at HEAD `{}` and now requires human architecture review.",
				self.run_id, self.issue_identifier, self.head_sha
			),
			ReviewPolicyStopReason::Blocked => write!(
				f,
				"Run `{}` for issue `{}` recorded `blocked` at HEAD `{}` and now requires human intervention.",
				self.run_id, self.issue_identifier, self.head_sha
			),
		}
	}
}

impl Error for ReviewPolicyStopRequested {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct PendingReviewAction {
	pub(super) pr_url: String,
	pub(super) summary: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct ScopeArgs {
	pub(super) issue_id: Option<String>,

	pub(super) issue_identifier: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TransitionArgs {
	#[serde(flatten)]
	pub(super) scope: ScopeArgs,
	pub(super) state: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CommentArgs {
	#[serde(flatten)]
	pub(super) scope: ScopeArgs,
	pub(super) kind: String,
	pub(super) error_class: Option<String>,
	pub(super) next_action: Option<String>,
	#[serde(default)]
	pub(super) blockers: Vec<String>,
	#[serde(default)]
	pub(super) evidence: Vec<String>,
	pub(super) failed_command: Option<String>,
	pub(super) raw_error: Option<String>,
	pub(super) summary: Option<String>,
	pub(super) decision_request: Option<AuthorityDecisionRequestArgs>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AuthorityDecisionRequestArgs {
	pub(super) boundary_check_id: i64,
	pub(super) decision_request_id: String,
	pub(super) reason_code: String,
	pub(super) boundary_type: String,
	pub(super) proposed_change: String,
	pub(super) why_exceeds_authority: String,
	#[serde(default)]
	pub(super) options: Vec<AuthorityDecisionOptionArgs>,
	pub(super) recommendation: String,
	pub(super) resume_condition: String,
	#[serde(default)]
	pub(super) retained_worktree_evidence: Vec<String>,
	#[serde(default)]
	pub(super) retained_diff_evidence: Vec<String>,
	#[serde(default)]
	pub(super) recovery_attempt_context: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AuthorityDecisionOptionArgs {
	pub(super) label: String,
	pub(super) description: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct ReviewHandoffArgs {
	#[serde(flatten)]
	pub(super) scope: ScopeArgs,
	pub(super) pr_url: String,
	pub(super) summary: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProgressCheckpointArgs {
	#[serde(flatten)]
	pub(super) scope: ScopeArgs,
	pub(super) phase: String,
	pub(super) docs_impact: String,
	pub(super) focus: String,
	pub(super) next_action: String,
	#[serde(default)]
	pub(super) blockers: Vec<String>,
	#[serde(default)]
	pub(super) evidence: Vec<String>,
	#[serde(default)]
	pub(super) verification: Vec<String>,
	pub(super) head_sha: Option<String>,
	pub(super) branch: Option<String>,
	pub(super) pr_url: Option<String>,
}

#[derive(Debug)]
pub(super) struct NormalizedProgressCheckpoint {
	pub(super) phase: ExecutionProgressPhase,
	pub(super) docs_impact: DocsImpact,
	pub(super) focus: String,
	pub(super) next_action: String,
	pub(super) blockers: Vec<String>,
	pub(super) evidence: Vec<String>,
	pub(super) verification: Vec<String>,
	pub(super) head_sha: Option<String>,
	pub(super) branch: Option<String>,
	pub(super) pr_url: Option<String>,
}
impl NormalizedProgressCheckpoint {
	pub(super) fn public_branch(&self, review_context: &ReviewHandoffContext) -> String {
		self.branch.clone().unwrap_or_else(|| review_context.branch_name.clone())
	}
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LabelArgs {
	#[serde(flatten)]
	pub(super) scope: ScopeArgs,
	pub(super) label: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct TerminalFinalizeArgs {
	#[serde(flatten)]
	pub(super) scope: ScopeArgs,
	pub(super) path: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReviewCheckpointArgs {
	#[serde(flatten)]
	pub(super) scope: ScopeArgs,
	pub(super) reviewer: Option<String>,
	pub(super) status: String,
	pub(super) head_sha: String,
	pub(super) review_contract: Option<ReviewCheckpointContractArgs>,
	pub(super) review_cost_control: Option<ReviewCostControlArgs>,
	pub(super) checks: Option<ReviewCheckpointChecksArgs>,
	#[serde(default)]
	pub(super) evidence: Vec<String>,
	#[serde(default)]
	pub(super) accepted_findings: Vec<ReviewCheckpointFindingArgs>,
	#[serde(default)]
	pub(super) rejected_findings: Vec<ReviewCheckpointRejectedFindingArgs>,
	#[serde(default)]
	pub(super) finding_routes: Vec<ReviewCheckpointFindingRouteArgs>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReviewCostControlArgs {
	pub(super) review_class: String,
	pub(super) risk_class: String,
	pub(super) changed_surface_count: u64,
	#[serde(default)]
	pub(super) changed_surface_summary: Vec<String>,
	#[serde(default)]
	pub(super) high_risk_surfaces: Vec<String>,
	pub(super) current_head_evidence: bool,
	pub(super) validation_backed: bool,
	#[serde(default)]
	pub(super) validation_current: bool,
	#[serde(default)]
	pub(super) evidence_sufficient: bool,
	pub(super) reviewer_judgment: String,
	pub(super) fallback_reason: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReviewCheckpointContractArgs {
	pub(super) workflow_policy_source: String,
	pub(super) review_type: String,
	pub(super) risk_tier: String,
	pub(super) objective: String,
	#[serde(default)]
	pub(super) scope: Vec<String>,
	#[serde(default)]
	pub(super) non_goals: Vec<String>,
	#[serde(default)]
	pub(super) required_checks: Vec<String>,
	#[serde(default)]
	pub(super) allowed_expansion_triggers: Vec<String>,
	#[serde(default)]
	pub(super) validation_evidence: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReviewCheckpointChecksArgs {
	pub(super) intended_behavior: String,
	pub(super) regression_risk: String,
	pub(super) missing_tests: String,
	pub(super) docs_config_drift: String,
	pub(super) migration_fallout: String,
	pub(super) operator_facing_fallout: String,
	pub(super) loop_decision_contract: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReviewCheckpointFindingArgs {
	pub(super) severity: String,
	pub(super) summary: String,
	#[serde(default)]
	pub(super) evidence: Vec<String>,
	pub(super) kind: Option<String>,
	pub(super) file: Option<String>,
	pub(super) line: Option<u64>,
	pub(super) line_range: Option<ReviewCheckpointLineRangeArgs>,
	pub(super) guidance: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReviewCheckpointRejectedFindingArgs {
	pub(super) severity: String,
	pub(super) summary: String,
	pub(super) rejection_reason: String,
	#[serde(default)]
	pub(super) evidence: Vec<String>,
	pub(super) kind: Option<String>,
	pub(super) file: Option<String>,
	pub(super) line: Option<u64>,
	pub(super) line_range: Option<ReviewCheckpointLineRangeArgs>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReviewCheckpointFindingRouteArgs {
	pub(super) route: String,
	pub(super) severity: String,
	pub(super) summary: String,
	#[serde(default)]
	pub(super) evidence: Vec<String>,
	pub(super) resolver: String,
	pub(super) next_action: String,
	pub(super) risk_tier: Option<String>,
	pub(super) finding_source: Option<String>,
	pub(super) finding_index: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReviewCheckpointLineRangeArgs {
	pub(super) start: u64,
	pub(super) end: u64,
}

#[derive(Debug, Serialize)]
pub(super) struct NormalizedReviewCheckpointPayload {
	pub(super) reviewer: String,
	pub(super) review_contract: NormalizedReviewCheckpointContract,
	pub(super) review_contract_hash: String,
	pub(super) review_cost_control: NormalizedReviewCostControl,
	pub(super) reviewed_head: ReviewCheckpointHeadBinding,
	pub(super) checks: ReviewCheckpointChecksArgs,
	pub(super) evidence: Vec<String>,
	pub(super) accepted_findings: Vec<NormalizedReviewCheckpointFinding>,
	pub(super) rejected_findings: Vec<NormalizedRejectedReviewCheckpointFinding>,
	pub(super) finding_routes: Vec<NormalizedReviewCheckpointFindingRoute>,
	pub(super) finding_route_summary: ReviewCheckpointFindingRouteSummary,
	pub(super) finding_policy: ReviewFindingPolicyState,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(super) struct NormalizedReviewCheckpointContract {
	pub(super) workflow_policy_source: String,
	pub(super) review_type: String,
	pub(super) risk_tier: String,
	pub(super) objective: String,
	pub(super) scope: Vec<String>,
	pub(super) non_goals: Vec<String>,
	pub(super) required_checks: Vec<String>,
	pub(super) allowed_expansion_triggers: Vec<String>,
	pub(super) validation_evidence: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(super) struct NormalizedReviewCostControl {
	pub(super) review_class: String,
	pub(super) risk_class: String,
	pub(super) compact_eligible: bool,
	pub(super) changed_surface_count: u64,
	pub(super) changed_surface_summary: Vec<String>,
	pub(super) high_risk_surfaces: Vec<String>,
	pub(super) current_head_evidence: bool,
	pub(super) validation_backed: bool,
	pub(super) validation_current: bool,
	pub(super) evidence_sufficient: bool,
	pub(super) reviewer_judgment: String,
	pub(super) fallback_reason: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(super) struct ReviewCheckpointHeadBinding {
	pub(super) head_sha: String,
	pub(super) head_tree_oid: String,
	pub(super) review_worktree_clean: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(super) struct NormalizedReviewCheckpointFinding {
	pub(super) severity: String,
	pub(super) summary: String,
	#[serde(default)]
	pub(super) evidence: Vec<String>,
	pub(super) kind: String,
	pub(super) file: Option<String>,
	pub(super) line: Option<u64>,
	pub(super) line_range: Option<ReviewCheckpointLineRangeArgs>,
	pub(super) guidance: String,
	pub(super) fingerprint: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(super) struct NormalizedRejectedReviewCheckpointFinding {
	pub(super) severity: String,
	pub(super) summary: String,
	pub(super) rejection_reason: String,
	#[serde(default)]
	pub(super) evidence: Vec<String>,
	pub(super) kind: String,
	pub(super) file: Option<String>,
	pub(super) line: Option<u64>,
	pub(super) line_range: Option<ReviewCheckpointLineRangeArgs>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(super) struct NormalizedReviewCheckpointFindingRoute {
	pub(super) route: String,
	pub(super) severity: String,
	pub(super) risk_tier: String,
	pub(super) summary: String,
	#[serde(default)]
	pub(super) evidence: Vec<String>,
	pub(super) resolver: String,
	pub(super) next_action: String,
	pub(super) finding_source: String,
	pub(super) finding_index: Option<u64>,
	pub(super) finding_fingerprint: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(super) struct ReviewCheckpointFindingRouteSummary {
	pub(super) route_counts: Vec<ReviewCheckpointFindingRouteCount>,
	pub(super) next_action: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(super) struct ReviewCheckpointFindingRouteCount {
	pub(super) route: String,
	pub(super) count: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
pub(super) struct ReviewFindingPolicyState {
	pub(super) schema: String,
	pub(super) phase: String,
	pub(super) status: String,
	pub(super) head_sha: String,
	pub(super) nonclean_rounds: i64,
	pub(super) active_fingerprints: Vec<String>,
	pub(super) stop_fingerprint: Option<String>,
	pub(super) findings: Vec<ReviewFindingPolicyRecord>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(super) struct ReviewFindingPolicyRecord {
	pub(super) fingerprint: String,
	pub(super) kind: String,
	pub(super) title: String,
	pub(super) body: String,
	pub(super) file: Option<String>,
	pub(super) line_range: Option<ReviewCheckpointLineRangeArgs>,
	pub(super) first_seen_head: String,
	pub(super) last_seen_head: String,
	pub(super) status: String,
	pub(super) repeat_count: i64,
	pub(super) repair_evidence: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ReviewPolicyState {
	pub(super) phase: ReviewPolicyPhase,
	pub(super) status: ReviewPolicyStatus,
	pub(super) head_sha: String,
	pub(super) nonclean_rounds: i64,
	pub(super) details_json: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReviewExecutionMode {
	Handoff,
	Repair,
	Closeout,
}
impl ReviewExecutionMode {
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::Handoff => "handoff",
			Self::Repair => "repair",
			Self::Closeout => "closeout",
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TurnCompletionStatus {
	Continue,
	Complete,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RunCompletionDisposition {
	ManualAttention,
	ReviewHandoff,
	ReviewRepair,
	Closeout,
}
impl RunCompletionDisposition {
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::ManualAttention => "manual_attention",
			Self::ReviewHandoff => "review_handoff",
			Self::ReviewRepair => "review_repair",
			Self::Closeout => "closeout",
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "type")]
pub(crate) enum DynamicToolContentItem {
	#[serde(rename = "inputText")]
	InputText { text: String },
}
impl DynamicToolContentItem {
	pub(super) fn text(text: String) -> Self {
		Self::InputText { text }
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReviewPolicyStopReason {
	Exhausted,
	ArchitectureReviewRequired,
	Blocked,
}
impl ReviewPolicyStopReason {
	pub(crate) fn error_class(self) -> &'static str {
		match self {
			Self::Exhausted => "review_policy_exhausted",
			Self::ArchitectureReviewRequired => "architecture_review_required",
			Self::Blocked => "review_policy_blocked",
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ExecutionProgressPhase {
	Probing,
	Implementing,
	Verifying,
	Blocked,
	ReadyForReview,
	ReviewRepair,
	ReadyToLand,
	Closeout,
}
impl ExecutionProgressPhase {
	pub(super) fn as_str(self) -> &'static str {
		match self {
			Self::Probing => "probing",
			Self::Implementing => "implementing",
			Self::Verifying => "verifying",
			Self::Blocked => "blocked",
			Self::ReadyForReview => "ready_for_review",
			Self::ReviewRepair => "review_repair",
			Self::ReadyToLand => "ready_to_land",
			Self::Closeout => "closeout",
		}
	}

	pub(super) fn parse(value: &str) -> std::result::Result<Self, String> {
		match value {
			"probing" => Ok(Self::Probing),
			"implementing" => Ok(Self::Implementing),
			"verifying" => Ok(Self::Verifying),
			"blocked" => Ok(Self::Blocked),
			"ready_for_review" => Ok(Self::ReadyForReview),
			"review_repair" => Ok(Self::ReviewRepair),
			"ready_to_land" => Ok(Self::ReadyToLand),
			"closeout" => Ok(Self::Closeout),
			other => Err(format!(
				"`issue_progress_checkpoint` phase must be `probing`, `implementing`, `verifying`, `blocked`, `ready_for_review`, `review_repair`, `ready_to_land`, or `closeout`, not `{other}`."
			)),
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DocsImpact {
	None,
	UpdateRequired,
	ResearchRequired,
	DriftRequired,
}
impl DocsImpact {
	pub(super) fn as_str(self) -> &'static str {
		match self {
			Self::None => "none",
			Self::UpdateRequired => "update_required",
			Self::ResearchRequired => "research_required",
			Self::DriftRequired => "drift_required",
		}
	}

	pub(super) fn parse(value: &str) -> std::result::Result<Self, String> {
		match value {
			"none" => Ok(Self::None),
			"update_required" => Ok(Self::UpdateRequired),
			"research_required" => Ok(Self::ResearchRequired),
			"drift_required" => Ok(Self::DriftRequired),
			other => Err(format!(
				"`issue_progress_checkpoint` docs_impact must be `none`, `update_required`, `research_required`, or `drift_required`, not `{other}`."
			)),
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ReviewPolicyPhase {
	Handoff,
	Repair,
}
impl ReviewPolicyPhase {
	pub(super) fn as_str(self) -> &'static str {
		match self {
			Self::Handoff => "handoff",
			Self::Repair => "repair",
		}
	}

	pub(super) fn for_mode(mode: ReviewExecutionMode) -> Option<Self> {
		match mode {
			ReviewExecutionMode::Handoff => Some(Self::Handoff),
			ReviewExecutionMode::Repair => Some(Self::Repair),
			ReviewExecutionMode::Closeout => None,
		}
	}

	pub(super) fn parse(value: &str) -> std::result::Result<Self, String> {
		match value {
			"handoff" => Ok(Self::Handoff),
			"repair" => Ok(Self::Repair),
			other => Err(format!(
				"Unsupported review policy phase `{other}` in the run activity marker."
			)),
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ReviewPolicyStatus {
	Clean,
	Findings,
	NeedsArchitectureReview,
	Blocked,
}
impl ReviewPolicyStatus {
	pub(super) fn as_str(self) -> &'static str {
		match self {
			Self::Clean => "clean",
			Self::Findings => "findings",
			Self::NeedsArchitectureReview => "needs_architecture_review",
			Self::Blocked => "blocked",
		}
	}

	pub(super) fn parse(value: &str) -> std::result::Result<Self, String> {
		match value {
			"clean" => Ok(Self::Clean),
			"findings" => Ok(Self::Findings),
			"needs_architecture_review" => Ok(Self::NeedsArchitectureReview),
			"blocked" => Ok(Self::Blocked),
			other => Err(format!(
				"`issue_review_checkpoint` status must be `clean`, `findings`, `needs_architecture_review`, or `blocked`, not `{other}`."
			)),
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum PendingReviewCompletion {
	Handoff(PendingReviewAction),
	Repair(PendingReviewAction),
	Closeout(PendingReviewAction),
}
