//! Linear issue mapping model for executable program nodes.

use serde::{Deserialize, Serialize};

use crate::{execution_program::validation, prelude::Result};

/// Normal Linear issue mapping for an executable program node.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct ExecutionLinearIssueMapping {
	pub(in crate::execution_program) issue_id: String,
	pub(in crate::execution_program) issue_identifier: String,
	pub(in crate::execution_program) issue_state: String,
	pub(in crate::execution_program) has_active_label: bool,
	pub(in crate::execution_program) has_opt_out_label: bool,
	pub(in crate::execution_program) has_needs_attention_label: bool,
	#[serde(default, skip_serializing_if = "validation::is_false")]
	pub(in crate::execution_program) has_open_tracker_blockers: bool,
	pub(in crate::execution_program) has_generic_dispatch_briefing: bool,
	#[serde(default, skip_serializing_if = "validation::is_false")]
	pub(in crate::execution_program) has_post_review_lifecycle: bool,
}
impl ExecutionLinearIssueMapping {
	/// Build a Linear issue mapping with no automation labels and a generic dispatch briefing.
	pub(crate) fn new(
		issue_id: impl Into<String>,
		issue_identifier: impl Into<String>,
		issue_state: impl Into<String>,
	) -> Result<Self> {
		let mapping = Self {
			issue_id: issue_id.into(),
			issue_identifier: issue_identifier.into(),
			issue_state: issue_state.into(),
			has_active_label: false,
			has_opt_out_label: false,
			has_needs_attention_label: false,
			has_open_tracker_blockers: false,
			has_generic_dispatch_briefing: true,
			has_post_review_lifecycle: false,
		};

		mapping.validate()?;

		Ok(mapping)
	}

	/// Mark whether the issue currently carries the service active label.
	pub(crate) fn with_active_label(mut self, present: bool) -> Self {
		self.has_active_label = present;

		self
	}

	/// Mark whether the issue currently carries the opt-out label.
	pub(crate) fn with_opt_out_label(mut self, present: bool) -> Self {
		self.has_opt_out_label = present;

		self
	}

	/// Mark whether the issue currently carries the needs-attention label.
	pub(crate) fn with_needs_attention_label(mut self, present: bool) -> Self {
		self.has_needs_attention_label = present;

		self
	}

	/// Mark whether the mapped issue currently has open tracker dependency blockers.
	pub(crate) fn with_open_tracker_blockers(mut self, present: bool) -> Self {
		self.has_open_tracker_blockers = present;

		self
	}

	/// Mark whether the issue description remains a generic dispatch briefing.
	pub(crate) fn with_generic_dispatch_briefing(mut self, present: bool) -> Self {
		self.has_generic_dispatch_briefing = present;

		self
	}

	/// Mark whether the mapped issue is owned by the retained post-review lifecycle.
	pub(crate) fn with_post_review_lifecycle(mut self, present: bool) -> Self {
		self.has_post_review_lifecycle = present;

		self
	}

	/// Linear issue identifier such as `XY-853`.
	pub(crate) fn issue_identifier(&self) -> &str {
		&self.issue_identifier
	}

	/// Linear issue id used by tracker APIs.
	pub(crate) fn issue_id(&self) -> &str {
		&self.issue_id
	}

	/// Tracker workflow state for the mapped issue.
	pub(crate) fn issue_state(&self) -> &str {
		&self.issue_state
	}

	/// Whether the service active label is currently present.
	pub(crate) fn has_active_label(&self) -> bool {
		self.has_active_label
	}

	/// Whether the configured opt-out label is currently present.
	pub(crate) fn has_opt_out_label(&self) -> bool {
		self.has_opt_out_label
	}

	/// Whether the configured human-attention label is currently present.
	pub(crate) fn has_needs_attention_label(&self) -> bool {
		self.has_needs_attention_label
	}

	/// Whether the mapped issue currently has open dependency blockers in the tracker.
	pub(crate) fn has_open_tracker_blockers(&self) -> bool {
		self.has_open_tracker_blockers
	}

	/// Whether the issue description is usable as a generic dispatch briefing.
	pub(crate) fn has_generic_dispatch_briefing(&self) -> bool {
		self.has_generic_dispatch_briefing
	}

	/// Whether Review & Landing currently owns the mapped issue.
	pub(crate) fn has_post_review_lifecycle(&self) -> bool {
		self.has_post_review_lifecycle
	}

	pub(super) fn validate(&self) -> Result<()> {
		validation::validate_required("execution program issue_mapping.issue_id", &self.issue_id)?;
		validation::validate_required(
			"execution program issue_mapping.issue_identifier",
			&self.issue_identifier,
		)?;
		validation::validate_required(
			"execution program issue_mapping.issue_state",
			&self.issue_state,
		)?;

		Ok(())
	}
}
