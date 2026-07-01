use std::cmp::Ordering;

use crate::state::ProjectRunStatus;

pub(in crate::state) fn compare_project_run_status(
	left: &ProjectRunStatus,
	right: &ProjectRunStatus,
) -> Ordering {
	right
		.run_lease
		.cmp(&left.run_lease)
		.then_with(|| right.updated_at.cmp(&left.updated_at))
		.then_with(|| right.attempt_number.cmp(&left.attempt_number))
		.then_with(|| right.run_id.cmp(&left.run_id))
}
