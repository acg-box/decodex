use crate::{
	agent::tracker_tool_bridge::{
		self, ISSUE_REVIEW_CHECKPOINT_TOOL_NAME, NormalizedReviewCheckpointContract,
		NormalizedReviewCostControl, ReviewCostControlArgs,
		tools::review_checkpoint::{
			REVIEW_CLASS_COMPACT_CURRENT_HEAD, REVIEW_CLASS_FULL_CURRENT_HEAD,
			REVIEW_COST_CONTROL_NOT_PROVIDED,
			normalize::{self, contract},
		},
	},
	tracker::public_text,
};

pub(in crate::agent::tracker_tool_bridge::tools::review_checkpoint::normalize) fn normalize_review_cost_control(
	cost_control: Option<ReviewCostControlArgs>,
	review_contract: &NormalizedReviewCheckpointContract,
) -> Result<NormalizedReviewCostControl, String> {
	let Some(cost_control) = cost_control else {
		return Ok(NormalizedReviewCostControl {
			review_class: String::from(REVIEW_CLASS_FULL_CURRENT_HEAD),
			risk_class: review_contract.risk_tier.clone(),
			compact_eligible: false,
			changed_surface_count: 0,
			changed_surface_summary: vec![String::from(
				"Review cost-control metadata was not supplied; standard full review remains required.",
			)],
			high_risk_surfaces: Vec::new(),
			current_head_evidence: false,
			validation_backed: false,
			validation_current: false,
			evidence_sufficient: false,
			reviewer_judgment: String::from(
				"No compact-review judgment was recorded; defaulting to full independent review.",
			),
			fallback_reason: Some(String::from(REVIEW_COST_CONTROL_NOT_PROVIDED)),
		});
	};
	let review_class = normalize_review_class(cost_control.review_class)?;
	let risk_class = contract::normalize_review_risk_tier(cost_control.risk_class)?;
	let changed_surface_summary = normalize_review_cost_control_list(
		cost_control.changed_surface_summary,
		"review_cost_control.changed_surface_summary",
		true,
	)?;
	let high_risk_surfaces = normalize_review_cost_control_list(
		cost_control.high_risk_surfaces,
		"review_cost_control.high_risk_surfaces",
		false,
	)?;
	let reviewer_judgment = normalize_public_review_cost_control_text(
		cost_control.reviewer_judgment,
		"review_cost_control.reviewer_judgment",
	)?;
	let fallback_reason = normalize_optional_public_review_cost_control_reason(
		cost_control.fallback_reason,
		"review_cost_control.fallback_reason",
	)?;
	let compact_eligible = review_class == REVIEW_CLASS_COMPACT_CURRENT_HEAD;

	if !compact_eligible && fallback_reason.is_none() {
		return Err(String::from(
			"`issue_review_checkpoint` requires `review_cost_control.fallback_reason` when `review_class` is `full_current_head_review`.",
		));
	}

	Ok(NormalizedReviewCostControl {
		review_class,
		risk_class,
		compact_eligible,
		changed_surface_count: cost_control.changed_surface_count,
		changed_surface_summary,
		high_risk_surfaces,
		current_head_evidence: cost_control.current_head_evidence,
		validation_backed: cost_control.validation_backed,
		validation_current: cost_control.validation_current,
		evidence_sufficient: cost_control.evidence_sufficient,
		reviewer_judgment,
		fallback_reason,
	})
}

fn normalize_review_class(review_class: String) -> Result<String, String> {
	let review_class = review_class.trim().to_ascii_lowercase().replace([' ', '-'], "_");
	let review_class = match review_class.as_str() {
		"compact" => REVIEW_CLASS_COMPACT_CURRENT_HEAD,
		"full" | "standard" => REVIEW_CLASS_FULL_CURRENT_HEAD,
		other => other,
	};

	match review_class {
		REVIEW_CLASS_COMPACT_CURRENT_HEAD | REVIEW_CLASS_FULL_CURRENT_HEAD =>
			Ok(review_class.to_owned()),
		other => {
			let compact = REVIEW_CLASS_COMPACT_CURRENT_HEAD;
			let full = REVIEW_CLASS_FULL_CURRENT_HEAD;

			Err(format!(
				"`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `review_cost_control.review_class` to be `{compact}` or `{full}`, not `{other}`."
			))
		},
	}
}

fn normalize_review_cost_control_list(
	values: Vec<String>,
	field_name: &str,
	required: bool,
) -> Result<Vec<String>, String> {
	let values = tracker_tool_bridge::normalize_progress_list(values)
		.into_iter()
		.map(|value| normalize_public_review_cost_control_text(value, field_name))
		.collect::<Result<Vec<_>, _>>()?;

	if required && values.is_empty() {
		return Err(format!("`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `{field_name}`."));
	}

	Ok(values)
}

fn normalize_public_review_cost_control_text(
	value: String,
	field_name: &str,
) -> Result<String, String> {
	let value = normalize::normalize_required_review_text(value, field_name)?;

	public_text::validate_public_text_field(field_name, &value)
		.map_err(|error| error.to_string())?;

	Ok(value)
}

fn normalize_optional_public_review_cost_control_reason(
	value: Option<String>,
	field_name: &str,
) -> Result<Option<String>, String> {
	let Some(value) = tracker_tool_bridge::normalize_optional_progress_field(value) else {
		return Ok(None);
	};
	let value = normalize_public_review_cost_control_text(value, field_name)?;

	Ok(Some(value))
}
