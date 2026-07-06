//! Tracker issue metadata hydration for operator status rows.

mod apply;
mod hydration;
mod missing;
mod selectors;

pub(in crate::orchestrator) use self::{
	apply::{fill_missing_history_lane_issue_metadata, fill_missing_run_issue_metadata},
	hydration::hydrate_operator_run_rows_from_tracker,
	selectors::{
		operator_run_is_stale_terminal_local_residue,
		operator_run_tracker_issue_identifier_selector,
	},
};
