mod envelope;
mod event_fields;
mod path;
mod public_text;

use crate::tracker::records::LinearExecutionEventRecord;

pub(crate) fn validate_linear_execution_event_record(
	record: &LinearExecutionEventRecord,
) -> Result<(), String> {
	envelope::validate_linear_execution_event_envelope(record)?;
	event_fields::validate_linear_execution_event_fields(record)?;
	public_text::validate_linear_execution_event_public_text(record)?;

	if let Some(worktree_path) = record.worktree_path.as_deref() {
		path::validate_repo_relative_path(worktree_path, "worktree_path")?;
	}

	Ok(())
}
