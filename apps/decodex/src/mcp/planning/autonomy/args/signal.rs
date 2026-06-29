use std::collections::BTreeMap;

use serde::Deserialize;

use crate::{
	autonomy_signal::{
		AutonomySignalConfidence, AutonomySignalEvidenceClass, AutonomySignalFreshness,
		AutonomySignalInput, AutonomySignalKind, AutonomySignalPrivacy,
		AutonomySignalReviewEvidence, AutonomySignalSourceType,
	},
	mcp::planning::{self, PlanningAuthorityArgs},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::mcp::planning::autonomy) struct AutonomySubmitSignalToolArgs {
	pub(in crate::mcp::planning::autonomy) mode: Option<String>,
	pub(in crate::mcp::planning::autonomy) project_id: Option<String>,
	pub(in crate::mcp::planning::autonomy) kind: AutonomySignalKind,
	pub(in crate::mcp::planning::autonomy) signal: AutonomySignalInputArgs,
	pub(in crate::mcp::planning::autonomy) authority: Option<PlanningAuthorityArgs>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::mcp::planning::autonomy) struct AutonomySignalInputArgs {
	pub(in crate::mcp::planning::autonomy) objective_id: String,
	pub(in crate::mcp::planning::autonomy) objective_version: u64,
	pub(in crate::mcp::planning::autonomy) source_type: AutonomySignalSourceType,
	pub(in crate::mcp::planning::autonomy) source_refs: Vec<String>,
	#[serde(default)]
	pub(in crate::mcp::planning::autonomy) primary_source_refs: Vec<String>,
	pub(in crate::mcp::planning::autonomy) issue_id: Option<String>,
	pub(in crate::mcp::planning::autonomy) run_id: Option<String>,
	pub(in crate::mcp::planning::autonomy) attempt_id: Option<String>,
	pub(in crate::mcp::planning::autonomy) head_sha: Option<String>,
	pub(in crate::mcp::planning::autonomy) captured_at: Option<String>,
	pub(in crate::mcp::planning::autonomy) freshness: AutonomySignalFreshness,
	pub(in crate::mcp::planning::autonomy) summary: String,
	pub(in crate::mcp::planning::autonomy) evidence: Vec<String>,
	pub(in crate::mcp::planning::autonomy) evidence_class: AutonomySignalEvidenceClass,
	#[serde(default)]
	pub(in crate::mcp::planning::autonomy) contradictions: Vec<String>,
	#[serde(default)]
	pub(in crate::mcp::planning::autonomy) gaps: Vec<String>,
	pub(in crate::mcp::planning::autonomy) confidence: AutonomySignalConfidence,
	pub(in crate::mcp::planning::autonomy) privacy: AutonomySignalPrivacy,
	#[serde(default)]
	pub(in crate::mcp::planning::autonomy) observed_counts: BTreeMap<String, u64>,
	pub(in crate::mcp::planning::autonomy) review_evidence: Option<AutonomySignalReviewEvidence>,
	pub(in crate::mcp::planning::autonomy) proposal_only: Option<bool>,
	pub(in crate::mcp::planning::autonomy) created_at: Option<String>,
}
impl AutonomySignalInputArgs {
	pub(in crate::mcp::planning::autonomy) fn into_signal_input(
		self,
		project_id: &str,
	) -> AutonomySignalInput {
		let now = planning::mcp_now_rfc3339();

		AutonomySignalInput {
			project_id: project_id.to_owned(),
			objective_id: self.objective_id,
			objective_version: self.objective_version,
			source_type: self.source_type,
			source_refs: self.source_refs,
			primary_source_refs: self.primary_source_refs,
			issue_id: self.issue_id,
			run_id: self.run_id,
			attempt_id: self.attempt_id,
			head_sha: self.head_sha,
			captured_at: self.captured_at.unwrap_or_else(|| now.clone()),
			freshness: self.freshness,
			summary: self.summary,
			evidence: self.evidence,
			evidence_class: self.evidence_class,
			contradictions: self.contradictions,
			gaps: self.gaps,
			confidence: self.confidence,
			privacy: self.privacy,
			observed_counts: self.observed_counts,
			review_evidence: self.review_evidence,
			proposal_only: self.proposal_only.unwrap_or(true),
			created_at: self.created_at.unwrap_or(now),
		}
	}
}
