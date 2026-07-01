use serde::{Deserialize, Serialize};

use crate::{
	loop_contract::{DecisionPromotionActorKind, validation},
	prelude::Result,
};

/// Promotion metadata that records who or what accepted the contract and when.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct DecisionPromotion {
	pub(super) accepted_by: String,
	pub(super) accepted_by_kind: DecisionPromotionActorKind,
	pub(super) accepted_at: String,
	pub(super) acceptance_source: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) promotion_reason: Option<String>,
}
#[allow(dead_code)]
impl DecisionPromotion {
	pub(crate) fn new(
		accepted_by: impl Into<String>,
		accepted_by_kind: DecisionPromotionActorKind,
		accepted_at: impl Into<String>,
		acceptance_source: impl Into<String>,
		promotion_reason: Option<String>,
	) -> Result<Self> {
		let promotion = Self {
			accepted_by: accepted_by.into(),
			accepted_by_kind,
			accepted_at: accepted_at.into(),
			acceptance_source: acceptance_source.into(),
			promotion_reason,
		};

		promotion.validate()?;

		Ok(promotion)
	}

	pub(crate) fn accepted_by(&self) -> &str {
		&self.accepted_by
	}

	pub(crate) fn accepted_at(&self) -> &str {
		&self.accepted_at
	}

	pub(super) fn validate(&self) -> Result<()> {
		validation::validate_required(
			"decision contract promotion.accepted_by",
			&self.accepted_by,
		)?;
		validation::validate_required(
			"decision contract promotion.accepted_at",
			&self.accepted_at,
		)?;
		validation::validate_required(
			"decision contract promotion.acceptance_source",
			&self.acceptance_source,
		)?;

		validation::validate_optional(
			"decision contract promotion.promotion_reason",
			self.promotion_reason.as_deref(),
		)
	}
}
