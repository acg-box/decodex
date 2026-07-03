mod due_dispatch;
mod due_releases;
mod future_claims;

use crate::{
	orchestrator::tests::{self, TEST_SERVICE_ID},
	tracker::{self, TrackerIssue},
};

fn selection_sample_service_owned_issue(state_name: &str) -> TrackerIssue {
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);

	tests::sample_issue(state_name, &[active_label.as_str()])
}

fn selection_sample_service_owned_issue_with_sort_fields(
	id: &str,
	identifier: &str,
	state_name: &str,
	sort_value: Option<i64>,
	updated_at: &str,
) -> TrackerIssue {
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);

	tests::sample_issue_with_sort_fields(
		id,
		identifier,
		state_name,
		&[active_label.as_str()],
		sort_value,
		updated_at,
	)
}

fn selection_sample_service_owned_issue_with_project_slug_and_sort_fields(
	id: &str,
	identifier: &str,
	project_slug: &str,
	state_name: &str,
	sort_value: Option<i64>,
	updated_at: &str,
) -> TrackerIssue {
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);

	tests::sample_issue_with_project_slug_and_sort_fields(
		id,
		identifier,
		project_slug,
		state_name,
		&[active_label.as_str()],
		sort_value,
		updated_at,
	)
}
