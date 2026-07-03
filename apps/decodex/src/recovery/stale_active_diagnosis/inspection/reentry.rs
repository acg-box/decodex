use crate::recovery::{
	stale_active_diagnosis::inspection::inputs::StaleActiveReleaseReentryInspection,
	stale_active_reentry::{self, StaleActiveReleaseReentryInput},
};

pub(super) fn apply_stale_active_release_reentry(
	inspection: StaleActiveReleaseReentryInspection<'_>,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) {
	stale_active_reentry::apply_stale_active_release_reentries(
		StaleActiveReleaseReentryInput {
			run: inspection.latest_run,
			run_lease: inspection.run_lease,
			active_shared_claim: inspection.active_shared_claim,
			labels: inspection.labels,
			issue: inspection.issue,
			tracker_policy: inspection.workflow.frontmatter().tracker(),
			worktree_state: inspection.worktree_state,
			control_channel: inspection.control_channel,
		},
		evidence,
		blockers,
	);
}
