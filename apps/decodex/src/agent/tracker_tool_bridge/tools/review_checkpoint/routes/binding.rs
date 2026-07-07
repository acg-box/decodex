use crate::agent::tracker_tool_bridge::{
	NormalizedRejectedReviewCheckpointFinding, NormalizedReviewCheckpointFinding,
	tools::review_checkpoint::{
		REVIEW_ROUTE_SOURCE_ACCEPTED, REVIEW_ROUTE_SOURCE_REJECTED, REVIEW_ROUTE_SOURCE_ROUTE_ONLY,
	},
};

pub(in crate::agent::tracker_tool_bridge::tools::review_checkpoint::routes) fn normalize_review_route_binding(
	source: &str,
	index: Option<u64>,
	accepted_findings: &[NormalizedReviewCheckpointFinding],
	rejected_findings: &[NormalizedRejectedReviewCheckpointFinding],
) -> Result<(Option<u64>, Option<String>), String> {
	match source {
		REVIEW_ROUTE_SOURCE_ACCEPTED => {
			let index = index.ok_or_else(|| {
				String::from(
					"`finding_routes.finding_index` is required when `finding_source` is `accepted_findings`.",
				)
			})?;
			let finding = accepted_findings
				.get(usize::try_from(index).map_err(|error| {
					format!("Failed to normalize accepted finding route index: {error}")
				})?)
				.ok_or_else(|| {
					format!(
						"`finding_routes.finding_index` `{index}` does not match any accepted finding."
					)
				})?;

			Ok((Some(index), Some(finding.fingerprint.clone())))
		},
		REVIEW_ROUTE_SOURCE_REJECTED => {
			let index = index.ok_or_else(|| {
				String::from(
					"`finding_routes.finding_index` is required when `finding_source` is `rejected_findings`.",
				)
			})?;

			rejected_findings
				.get(usize::try_from(index).map_err(|error| {
					format!("Failed to normalize rejected finding route index: {error}")
				})?)
				.ok_or_else(|| {
					format!(
						"`finding_routes.finding_index` `{index}` does not match any rejected finding."
					)
				})?;

			Ok((Some(index), None))
		},
		REVIEW_ROUTE_SOURCE_ROUTE_ONLY => {
			if index.is_some() {
				return Err(String::from(
					"`finding_routes.finding_index` is only valid with `accepted_findings` or `rejected_findings` sources.",
				));
			}

			Ok((None, None))
		},
		_ => Err(String::from(
			"`finding_routes.finding_source` did not normalize to a supported source.",
		)),
	}
}

pub(in crate::agent::tracker_tool_bridge::tools::review_checkpoint::routes) fn review_route_bound_finding_severity<
	'a,
>(
	source: &str,
	index: Option<u64>,
	accepted_findings: &'a [NormalizedReviewCheckpointFinding],
	rejected_findings: &'a [NormalizedRejectedReviewCheckpointFinding],
) -> Option<&'a str> {
	let index = usize::try_from(index?).ok()?;

	match source {
		REVIEW_ROUTE_SOURCE_ACCEPTED => {
			accepted_findings.get(index).map(|finding| finding.severity.as_str())
		},
		REVIEW_ROUTE_SOURCE_REJECTED => {
			rejected_findings.get(index).map(|finding| finding.severity.as_str())
		},
		_ => None,
	}
}
