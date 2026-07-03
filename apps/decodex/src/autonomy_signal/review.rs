//! Review-derived autonomy signal evidence.

use serde::{Deserialize, Serialize};

use crate::{
	autonomy_signal::validation::{self},
	prelude::{Result, eyre},
};

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AutonomySignalReviewRoute {
	pub(crate) route: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) finding_source: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) finding_index: Option<u64>,
	pub(crate) summary: String,
	#[serde(default)]
	pub(crate) evidence_refs: Vec<String>,
}
impl AutonomySignalReviewRoute {
	pub(super) fn validate(&self) -> Result<()> {
		validation::validate_required("autonomy signal review route.route", &self.route)?;
		validation::validate_required("autonomy signal review route.summary", &self.summary)?;
		validation::validate_optional_required(
			"autonomy signal review route.finding_source",
			self.finding_source.as_deref(),
		)?;

		validation::validate_string_list(
			"autonomy signal review route.evidence_refs",
			&self.evidence_refs,
		)
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AutonomySignalReviewEvidence {
	pub(crate) review_phase: String,
	pub(crate) review_status: String,
	pub(crate) head_sha: String,
	#[serde(default)]
	pub(crate) checkpoint_refs: Vec<String>,
	#[serde(default)]
	pub(crate) finding_routes: Vec<AutonomySignalReviewRoute>,
}
impl AutonomySignalReviewEvidence {
	pub(super) fn validate(&self) -> Result<()> {
		validation::validate_required(
			"autonomy signal review evidence.review_phase",
			&self.review_phase,
		)?;
		validation::validate_required(
			"autonomy signal review evidence.review_status",
			&self.review_status,
		)?;
		validation::validate_required("autonomy signal review evidence.head_sha", &self.head_sha)?;
		validation::validate_nonempty_list(
			"autonomy signal review evidence.checkpoint_refs",
			&self.checkpoint_refs,
		)?;

		if self.finding_routes.is_empty() {
			eyre::bail!("Review-derived autonomy signals require finding_routes.");
		}

		for route in &self.finding_routes {
			route.validate()?;
		}

		Ok(())
	}
}
