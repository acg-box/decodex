use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AutonomyProposalChallengeSource {
	#[serde(alias = "support_agent")]
	Subagent,
	InlineSkeptic,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AutonomyProposalChallengeInput {
	pub(crate) source: AutonomyProposalChallengeSource,
	pub(crate) actor: String,
	pub(crate) summary: String,
	pub(crate) objections: Vec<String>,
	pub(crate) evidence_refs: Vec<String>,
	pub(crate) recorded_at: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AutonomyProposalChallengeEvidence {
	pub(in crate::autonomy_proposal) source: AutonomyProposalChallengeSource,
	pub(in crate::autonomy_proposal) actor: String,
	pub(in crate::autonomy_proposal) summary: String,
	#[serde(default)]
	pub(in crate::autonomy_proposal) objections: Vec<String>,
	#[serde(default)]
	pub(in crate::autonomy_proposal) evidence_refs: Vec<String>,
	pub(in crate::autonomy_proposal) recorded_at: String,
	pub(in crate::autonomy_proposal) acceptance_authority: bool,
}
