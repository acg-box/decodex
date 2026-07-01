//! Missing-issue ghost-lane cleanup status projection.

mod blockers;
mod conditions;
mod projection;
mod tracker_issue;

pub(crate) use self::{
	blockers::ghost_lane_cleanup_status_blockers,
	projection::apply_missing_issue_ghost_lane_projection,
	tracker_issue::mark_operator_run_tracker_issue_missing,
};
