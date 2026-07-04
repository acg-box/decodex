mod post_review;
mod push;
mod sort;

use std::path::Path;

use crate::orchestrator::agent_evidence::{
	AgentBlocker, AgentEvidenceProjectView, AgentRunCapsuleRef,
};

pub(crate) fn build_agent_blockers(
	project_view: &AgentEvidenceProjectView<'_>,
	blockers_dir: &Path,
	run_refs: &[AgentRunCapsuleRef],
) -> Vec<AgentBlocker> {
	let mut blockers = Vec::new();

	push::push_run_blockers(&mut blockers, project_view, blockers_dir, run_refs);
	push::push_queued_candidate_blockers(&mut blockers, project_view, blockers_dir, run_refs);
	push::push_post_review_lane_blockers(&mut blockers, project_view, blockers_dir);
	push::push_recovery_worktree_blockers(&mut blockers, project_view, blockers_dir);
	push::push_warning_blockers(&mut blockers, project_view, blockers_dir);
	push::push_connector_backoff_blockers(&mut blockers, project_view, blockers_dir);
	sort::sort_agent_blockers(&mut blockers);

	blockers
}
