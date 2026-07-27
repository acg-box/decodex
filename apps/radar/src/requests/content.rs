use std::path::PathBuf;

use serde::Serialize;

/// Request to prove one queue subject is eligible for content consideration.
#[derive(Debug)]
pub(crate) struct RadarContentEligibilityRequest {
	/// Upstream review queue containing the selected subject.
	pub(crate) queue: PathBuf,
	/// One source-backed `upstream_review/v1` artifact.
	pub(crate) review: PathBuf,
	/// One matching `upstream_impact/v1` artifact.
	pub(crate) impact: PathBuf,
	/// Maximum age for all three source artifacts.
	pub(crate) max_age_hours: u64,
}

/// Successful one-subject content eligibility proof.
#[derive(Debug, Eq, PartialEq, Serialize)]
pub(crate) struct RadarContentEligibilityReport {
	pub(crate) schema: String,
	pub(crate) repo: String,
	pub(crate) subject_kind: String,
	pub(crate) subject_id: String,
	pub(crate) slug: String,
	pub(crate) upstream_head: String,
	pub(crate) commit_shas: Vec<String>,
	pub(crate) queue_sha256: String,
	pub(crate) review_sha256: String,
	pub(crate) impact_sha256: String,
	pub(crate) lineage_sha256: String,
}

/// Request to select at most one queued subject for source review.
#[derive(Debug)]
pub(crate) struct RadarReviewNextRequest {
	/// Owner-only Radar cache root.
	pub(crate) cache_root: PathBuf,
	/// Maximum age for the queue snapshot.
	pub(crate) max_age_hours: u64,
}

/// Bounded identity for one selected queue subject.
#[derive(Debug, Eq, PartialEq, Serialize)]
pub(crate) struct RadarSelectedSubject {
	pub(crate) repo: String,
	pub(crate) subject_kind: String,
	pub(crate) subject_id: String,
	pub(crate) slug: String,
	pub(crate) title: String,
	pub(crate) source_state: String,
	pub(crate) commit_shas: Vec<String>,
}

/// Immutable identity of the queue generation used for selection.
#[derive(Debug, Eq, PartialEq, Serialize)]
pub(crate) struct RadarQueueGeneration {
	#[serde(rename = "ref")]
	pub(crate) queue_ref: String,
	pub(crate) sha256: String,
	pub(crate) generated_at: String,
	pub(crate) upstream_head: String,
}

/// Bounded upstream source reference for the native source-review pass.
#[derive(Debug, Eq, PartialEq, Serialize)]
pub(crate) struct RadarSourceRef {
	pub(crate) kind: String,
	pub(crate) title: String,
	pub(crate) url: String,
}

/// Result of one deterministic source-review selection attempt.
#[derive(Debug, Eq, PartialEq, Serialize)]
pub(crate) struct RadarReviewNextReport {
	pub(crate) schema: String,
	pub(crate) status: String,
	pub(crate) selected: Option<RadarSelectedSubject>,
	pub(crate) queue_generation: RadarQueueGeneration,
	pub(crate) source_refs: Vec<RadarSourceRef>,
	pub(crate) selection_sha256: Option<String>,
}
