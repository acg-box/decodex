use std::collections::BTreeMap;

use crate::orchestrator::harness_improvement::{HarnessImprovementCandidateSummary, Value};

pub(in crate::orchestrator::harness_improvement) fn json_string(
	value: Option<&Value>,
) -> Option<String> {
	value.and_then(Value::as_str).filter(|value| !value.is_empty()).map(str::to_owned)
}

pub(in crate::orchestrator::harness_improvement) fn json_array_len(value: Option<&Value>) -> usize {
	value.and_then(Value::as_array).map_or(0, Vec::len)
}

pub(in crate::orchestrator::harness_improvement::candidates) fn insert_candidate(
	candidates: &mut BTreeMap<String, HarnessImprovementCandidateSummary>,
	kind: &str,
	reason_code: &str,
	target: &str,
	source_event_count: usize,
	recommendation: &str,
) {
	let key = format!("{kind}:{reason_code}:{target}");

	candidates.entry(key).or_insert_with(|| HarnessImprovementCandidateSummary {
		kind: kind.to_owned(),
		reason_code: reason_code.to_owned(),
		target: target.to_owned(),
		source_event_count,
		recommendation: recommendation.to_owned(),
	});
}
