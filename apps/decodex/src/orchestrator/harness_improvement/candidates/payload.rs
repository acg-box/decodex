use crate::orchestrator::harness_improvement::{
	HarnessImprovementCandidateSummary, Value, candidates::util,
};

pub(in crate::orchestrator::harness_improvement) fn authority_boundary_final_reason_mentions_underspecified(
	payload: &Value,
) -> bool {
	let reason = payload
		.get("final_disposition")
		.and_then(|value| util::json_string(value.get("reason")))
		.or_else(|| util::json_string(payload.get("final_disposition_reason")));

	reason.is_some_and(|reason| {
		let reason = reason.to_ascii_lowercase();

		reason.contains("underspecified")
			|| reason.contains("missing contract")
			|| reason.contains("missing authority")
	})
}

pub(in crate::orchestrator::harness_improvement) fn first_decision_contract_target(
	payload: &Value,
) -> Option<String> {
	payload
		.get("decision_contract_ids")
		.and_then(Value::as_array)?
		.iter()
		.filter_map(Value::as_str)
		.find(|contract_id| !contract_id.is_empty())
		.map(|contract_id| format!("decision_contract:{contract_id}"))
}

pub(in crate::orchestrator::harness_improvement) fn harness_candidates_from_payload(
	payload: &Value,
) -> Vec<HarnessImprovementCandidateSummary> {
	payload
		.get("improvement_candidates")
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
		.filter_map(|candidate| {
			Some(HarnessImprovementCandidateSummary {
				kind: util::json_string(candidate.get("kind"))?,
				reason_code: util::json_string(candidate.get("reason_code"))?,
				target: util::json_string(candidate.get("target"))?,
				source_event_count: candidate
					.get("source_event_count")
					.and_then(Value::as_u64)
					.and_then(|value| usize::try_from(value).ok())
					.unwrap_or(0),
				recommendation: util::json_string(candidate.get("recommendation"))?,
			})
		})
		.collect()
}
