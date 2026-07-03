use crate::{
	prelude::Result,
	recovery::{
		stale_active_authority,
		stale_active_diagnosis::inspection::StaleActiveAuthorityEvidenceInspection,
	},
	tracker::IssueTracker,
};

pub(super) fn inspect_stale_active_authority_evidence<T>(
	inspection: StaleActiveAuthorityEvidenceInspection<'_, T>,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	stale_active_authority::inspect_stale_active_private_evidence(
		inspection.project_id,
		inspection.state_store,
		inspection.issue_keys,
		inspection.latest_run,
		inspection.marker_liveness,
		evidence,
		blockers,
	)?;

	stale_active_authority::inspect_stale_active_review_lineage(
		inspection.project_id,
		inspection.state_store,
		inspection.tracker,
		inspection.issue,
		evidence,
		blockers,
	)
}
