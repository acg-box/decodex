use super::review::ReviewHandoffContext;

#[derive(Debug)]
pub(in crate::agent::tracker_tool_bridge) struct NormalizedProgressCheckpoint {
	pub(in crate::agent::tracker_tool_bridge) phase: ExecutionProgressPhase,
	pub(in crate::agent::tracker_tool_bridge) docs_impact: DocsImpact,
	pub(in crate::agent::tracker_tool_bridge) focus: String,
	pub(in crate::agent::tracker_tool_bridge) next_action: String,
	pub(in crate::agent::tracker_tool_bridge) blockers: Vec<String>,
	pub(in crate::agent::tracker_tool_bridge) evidence: Vec<String>,
	pub(in crate::agent::tracker_tool_bridge) verification: Vec<String>,
	pub(in crate::agent::tracker_tool_bridge) head_sha: Option<String>,
	pub(in crate::agent::tracker_tool_bridge) branch: Option<String>,
	pub(in crate::agent::tracker_tool_bridge) pr_url: Option<String>,
}
impl NormalizedProgressCheckpoint {
	pub(in crate::agent::tracker_tool_bridge) fn public_branch(
		&self,
		review_context: &ReviewHandoffContext,
	) -> String {
		self.branch.clone().unwrap_or_else(|| review_context.branch_name.clone())
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::agent::tracker_tool_bridge) enum ExecutionProgressPhase {
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
	pub(in crate::agent::tracker_tool_bridge) fn as_str(self) -> &'static str {
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

	pub(in crate::agent::tracker_tool_bridge) fn parse(
		value: &str,
	) -> std::result::Result<Self, String> {
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
pub(in crate::agent::tracker_tool_bridge) enum DocsImpact {
	None,
	UpdateRequired,
	ResearchRequired,
	DriftRequired,
}
impl DocsImpact {
	pub(in crate::agent::tracker_tool_bridge) fn as_str(self) -> &'static str {
		match self {
			Self::None => "none",
			Self::UpdateRequired => "update_required",
			Self::ResearchRequired => "research_required",
			Self::DriftRequired => "drift_required",
		}
	}

	pub(in crate::agent::tracker_tool_bridge) fn parse(
		value: &str,
	) -> std::result::Result<Self, String> {
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
