use std::collections::HashSet;

pub(in crate::state::project_run_recovery::collect::sources) fn project_recovery_record_is_out_of_scope(
	project_id: &str,
	issue_id: Option<&str>,
	recorded_run_ids: &HashSet<String>,
	record_project_id: &str,
	record_issue_id: &str,
	record_run_id: &str,
) -> bool {
	record_project_id != project_id
		|| issue_id.is_some_and(|issue_id| record_issue_id != issue_id)
		|| recorded_run_ids.contains(record_run_id)
}
