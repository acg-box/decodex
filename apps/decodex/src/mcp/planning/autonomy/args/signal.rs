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
pub(in crate::mcp) struct AutonomySubmitSignalToolArgs {
	pub(in crate::mcp) mode: Option<String>,
	pub(in crate::mcp) project_id: Option<String>,
	pub(in crate::mcp) kind: AutonomySignalKind,
	pub(in crate::mcp) signal: AutonomySignalInputArgs,
	pub(in crate::mcp) authority: Option<PlanningAuthorityArgs>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::mcp) struct AutonomySignalInputArgs {
	pub(in crate::mcp) objective_id: String,
	pub(in crate::mcp) objective_version: u64,
	pub(in crate::mcp) source_type: AutonomySignalSourceType,
	pub(in crate::mcp) source_refs: Vec<String>,
	#[serde(default)]
	pub(in crate::mcp) primary_source_refs: Vec<String>,
	pub(in crate::mcp) issue_id: Option<String>,
	pub(in crate::mcp) run_id: Option<String>,
	pub(in crate::mcp) attempt_id: Option<String>,
	pub(in crate::mcp) head_sha: Option<String>,
	pub(in crate::mcp) captured_at: Option<String>,
	pub(in crate::mcp) freshness: AutonomySignalFreshness,
	pub(in crate::mcp) summary: String,
	pub(in crate::mcp) evidence: Vec<String>,
	pub(in crate::mcp) evidence_class: AutonomySignalEvidenceClass,
	#[serde(default)]
	pub(in crate::mcp) contradictions: Vec<String>,
	#[serde(default)]
	pub(in crate::mcp) gaps: Vec<String>,
	pub(in crate::mcp) confidence: AutonomySignalConfidence,
	pub(in crate::mcp) privacy: AutonomySignalPrivacy,
	#[serde(default)]
	pub(in crate::mcp) observed_counts: BTreeMap<String, u64>,
	pub(in crate::mcp) review_evidence: Option<AutonomySignalReviewEvidence>,
	pub(in crate::mcp) proposal_only: Option<bool>,
	pub(in crate::mcp) created_at: Option<String>,
}
impl AutonomySignalInputArgs {
	pub(in crate::mcp) fn into_signal_input(self, project_id: &str) -> AutonomySignalInput {
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
