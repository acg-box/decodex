use crate::orchestrator::agent_evidence::AgentBlocker;

pub(crate) fn sort_agent_blockers(blockers: &mut [AgentBlocker]) {
	blockers.sort_by(|left, right| {
		left.issue_identifier
			.cmp(&right.issue_identifier)
			.then_with(|| left.issue_id.cmp(&right.issue_id))
			.then_with(|| left.surface.cmp(&right.surface))
			.then_with(|| left.reason_code.cmp(&right.reason_code))
	});
}
