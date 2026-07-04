use std::collections::BTreeSet;

use crate::orchestrator::harness_improvement::{
	HarnessLinearProjectionSummary, LinearExecutionEventRecord,
};

pub(super) fn harness_linear_projection(
	linear_records: &[LinearExecutionEventRecord],
) -> HarnessLinearProjectionSummary {
	let mut event_types =
		linear_records.iter().map(|record| record.event_type.clone()).collect::<Vec<_>>();

	event_types.sort();
	event_types.dedup();

	let final_record = linear_records
		.iter()
		.max_by(|left, right| left.event_timestamp.cmp(&right.event_timestamp));

	HarnessLinearProjectionSummary {
		event_types,
		final_event_type: final_record.map(|record| record.event_type.clone()),
		final_error_class: final_record.and_then(|record| record.error_class.clone()),
		final_terminal_path: final_record.and_then(|record| record.terminal_path.clone()),
	}
}

pub(super) fn harness_pr_urls(
	explicit_pr_url: Option<&str>,
	linear_records: &[LinearExecutionEventRecord],
) -> Vec<String> {
	let mut pr_urls = explicit_pr_url.into_iter().map(str::to_owned).collect::<BTreeSet<_>>();

	for record in linear_records {
		if let Some(pr_url) = &record.pr_url {
			pr_urls.insert(pr_url.clone());
		}
	}

	pr_urls.into_iter().collect()
}
