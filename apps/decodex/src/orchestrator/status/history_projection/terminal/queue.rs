use std::collections::HashSet;

use crate::orchestrator::{OperatorStatusSnapshot, status_history_projection::predicates};

pub(crate) fn suppress_terminal_attention_queue_echoes(snapshot: &mut OperatorStatusSnapshot) {
	let terminal_attention_keys = snapshot
		.history_lanes
		.iter()
		.filter(|lane| predicates::history_ledger_outcome_requires_attention(&lane.ledger_outcome))
		.map(predicates::history_lane_group_key)
		.collect::<HashSet<_>>();

	if terminal_attention_keys.is_empty() {
		return;
	}

	snapshot.queued_candidates.retain(|candidate| {
		let candidate_key = predicates::terminal_attention_queue_key(
			&candidate.issue_id,
			&candidate.issue_identifier,
		);
		let is_terminal_attention_echo = candidate.reason == "issue_needs_attention"
			&& terminal_attention_keys.contains(&candidate_key);

		!is_terminal_attention_echo
	});
}
