use super::review::ReviewHandoffContext;

#[derive(Debug)]
pub(crate) struct NormalizedProgressCheckpoint {
	pub(crate) phase: ExecutionProgressPhase,
	pub(crate) docs_impact: DocsImpact,
	pub(crate) focus: String,
	pub(crate) next_action: String,
	pub(crate) blockers: Vec<String>,
	pub(crate) evidence: Vec<String>,
	pub(crate) verification: Vec<String>,
	pub(crate) head_sha: Option<String>,
	pub(crate) branch: Option<String>,
	pub(crate) pr_url: Option<String>,
}
impl NormalizedProgressCheckpoint {
	pub(crate) fn public_branch(&self, review_context: &ReviewHandoffContext) -> String {
		self.branch.clone().unwrap_or_else(|| review_context.branch_name.clone())
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExecutionProgressPhase {
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
	pub(crate) fn as_str(self) -> &'static str {
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

	pub(crate) fn parse(value: &str) -> std::result::Result<Self, String> {
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
pub(crate) enum DocsImpact {
	None,
	UpdateRequired,
	ResearchRequired,
	DriftRequired,
}
impl DocsImpact {
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::None => "none",
			Self::UpdateRequired => "update_required",
			Self::ResearchRequired => "research_required",
			Self::DriftRequired => "drift_required",
		}
	}

	pub(crate) fn parse(value: &str) -> std::result::Result<Self, String> {
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
