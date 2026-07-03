//! Autonomy signal payload and validation.

mod accessors;
mod constructor;
mod validation_impl;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::autonomy_signal::{
	review::AutonomySignalReviewEvidence,
	types::{
		AutonomySignalConfidence, AutonomySignalEvidenceClass, AutonomySignalFreshness,
		AutonomySignalKind, AutonomySignalPrivacy, AutonomySignalSourceType,
	},
};

pub(crate) const AUTONOMY_SIGNAL_SCHEMA: &str = "decodex.autonomy_signal/1";

const AUTONOMY_SIGNAL_RECORD_VERSION: u16 = 1;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AutonomySignalInput {
	pub(crate) project_id: String,
	pub(crate) objective_id: String,
	pub(crate) objective_version: u64,
	pub(crate) source_type: AutonomySignalSourceType,
	pub(crate) source_refs: Vec<String>,
	pub(crate) primary_source_refs: Vec<String>,
	pub(crate) issue_id: Option<String>,
	pub(crate) run_id: Option<String>,
	pub(crate) attempt_id: Option<String>,
	pub(crate) head_sha: Option<String>,
	pub(crate) captured_at: String,
	pub(crate) freshness: AutonomySignalFreshness,
	pub(crate) summary: String,
	pub(crate) evidence: Vec<String>,
	pub(crate) evidence_class: AutonomySignalEvidenceClass,
	pub(crate) contradictions: Vec<String>,
	pub(crate) gaps: Vec<String>,
	pub(crate) confidence: AutonomySignalConfidence,
	pub(crate) privacy: AutonomySignalPrivacy,
	pub(crate) observed_counts: BTreeMap<String, u64>,
	pub(crate) review_evidence: Option<AutonomySignalReviewEvidence>,
	pub(crate) proposal_only: bool,
	pub(crate) created_at: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AutonomySignal {
	#[serde(default = "autonomy_signal_schema")]
	pub(super) schema: String,
	#[serde(default = "autonomy_signal_record_version")]
	pub(super) record_version: u16,
	pub(super) id: String,
	pub(super) fingerprint: String,
	pub(super) project_id: String,
	pub(super) objective_id: String,
	pub(super) objective_version: u64,
	pub(super) kind: AutonomySignalKind,
	pub(super) source_type: AutonomySignalSourceType,
	pub(super) source_refs: Vec<String>,
	#[serde(default)]
	pub(super) primary_source_refs: Vec<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) issue_id: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) run_id: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) attempt_id: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) head_sha: Option<String>,
	pub(super) captured_at: String,
	pub(super) freshness: AutonomySignalFreshness,
	pub(super) summary: String,
	pub(super) evidence: Vec<String>,
	pub(super) evidence_class: AutonomySignalEvidenceClass,
	#[serde(default)]
	pub(super) contradictions: Vec<String>,
	#[serde(default)]
	pub(super) gaps: Vec<String>,
	pub(super) confidence: AutonomySignalConfidence,
	pub(super) privacy: AutonomySignalPrivacy,
	#[serde(default)]
	pub(super) observed_counts: BTreeMap<String, u64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) review_evidence: Option<AutonomySignalReviewEvidence>,
	pub(super) proposal_only: bool,
	pub(super) created_at: String,
}
pub(in crate::autonomy_signal::model) fn autonomy_signal_schema() -> String {
	AUTONOMY_SIGNAL_SCHEMA.to_owned()
}

pub(in crate::autonomy_signal::model) const fn autonomy_signal_record_version() -> u16 {
	AUTONOMY_SIGNAL_RECORD_VERSION
}
