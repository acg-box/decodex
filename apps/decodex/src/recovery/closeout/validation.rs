mod legacy;
mod merged;
mod superseded;

pub(in crate::recovery::closeout) use self::superseded::{
	ensure_superseded_closeout_run_attempt_compatible, ensure_superseded_issue_terminalizable,
	validate_obsolete_pull_request_closed, validate_obsolete_pull_request_unchanged,
};
pub(super) use self::{
	legacy::validate_legacy_closeout_request, merged::validate_merged_closeout_request,
	superseded::validate_superseded_closeout_request,
};
