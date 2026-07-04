use std::collections::BTreeMap;

use crate::state::{StateData, project_run_recovery::candidate::ProjectRunRecoveryCandidate};

pub(in crate::state::project_run_recovery::collect) fn finalize_project_run_recovery_candidates(
	state: &StateData,
	candidates: &mut BTreeMap<String, ProjectRunRecoveryCandidate>,
) {
	for candidate in candidates.values_mut() {
		if let Some(summary) = state.run_activity_summaries.get(&candidate.run_id) {
			let status = candidate.status.clone();

			candidate.merge(
				summary.attempt_number,
				&status,
				summary.updated_at.clone(),
				summary.updated_at_unix,
				String::from("run_activity_summary"),
			);
		}

		let event_summary = state.protocol_event_summary(&candidate.run_id);

		if event_summary.event_count > 0 {
			candidate.evidence.insert(format!("protocol_events:{}", event_summary.event_count));
		}
		if !state.run_activity_summaries.contains_key(&candidate.run_id)
			&& event_summary.event_count == 0
		{
			candidate.gaps.insert(String::from("no_activity_or_protocol_summary"));
		}
	}
}
