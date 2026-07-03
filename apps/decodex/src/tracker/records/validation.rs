use std::path::{Component, Path};

use crate::tracker::{
	public_text,
	records::{
		LINEAR_EXECUTION_EVENT_RECORD_TYPE, LINEAR_EXECUTION_EVENT_RECORD_VERSION,
		LinearExecutionEventRecord,
	},
};

pub(crate) fn validate_linear_execution_event_record(
	record: &LinearExecutionEventRecord,
) -> Result<(), String> {
	validate_linear_execution_event_envelope(record)?;
	validate_linear_execution_event_fields(record)?;
	validate_linear_execution_event_public_text(record)?;

	if let Some(worktree_path) = record.worktree_path.as_deref() {
		validate_repo_relative_path(worktree_path, "worktree_path")?;
	}

	Ok(())
}

fn validate_linear_execution_event_public_text(
	record: &LinearExecutionEventRecord,
) -> Result<(), String> {
	for (field_name, value) in [
		("summary", record.summary.as_deref()),
		("focus", record.focus.as_deref()),
		("next_action", record.next_action.as_deref()),
		("failed_command", record.failed_command.as_deref()),
		("raw_error", record.raw_error.as_deref()),
	] {
		if let Some(value) = value {
			public_text::validate_public_text_field(field_name, value)?;
		}
	}
	for (field_name, values) in [
		("blockers", record.blockers.as_ref()),
		("evidence", record.evidence.as_ref()),
		("verification", record.verification.as_ref()),
	] {
		if let Some(values) = values {
			public_text::validate_public_text_items(field_name, values)?;
		}
	}

	Ok(())
}

fn validate_linear_execution_event_envelope(
	record: &LinearExecutionEventRecord,
) -> Result<(), String> {
	if record.record_type != LINEAR_EXECUTION_EVENT_RECORD_TYPE {
		return Err(format!("`record_type` must be `{LINEAR_EXECUTION_EVENT_RECORD_TYPE}`."));
	}
	if record.record_version != LINEAR_EXECUTION_EVENT_RECORD_VERSION {
		return Err(format!("`record_version` must be `{LINEAR_EXECUTION_EVENT_RECORD_VERSION}`."));
	}

	for (field, value) in [
		("event_type", record.event_type.as_str()),
		("event_timestamp", record.event_timestamp.as_str()),
		("idempotency_key", record.idempotency_key.as_str()),
		("service_id", record.service_id.as_str()),
		("issue_id", record.issue_id.as_str()),
		("issue_identifier", record.issue_identifier.as_str()),
		("run_id", record.run_id.as_str()),
	] {
		if value.trim().is_empty() {
			return Err(format!("`{field}` must not be empty."));
		}
	}

	if record.attempt_number < 1 {
		return Err(String::from("`attempt_number` must be at least 1."));
	}

	Ok(())
}

fn validate_linear_execution_event_fields(
	record: &LinearExecutionEventRecord,
) -> Result<(), String> {
	match record.event_type.as_str() {
		"run_started" => {
			require_string(record.branch.as_deref(), "branch")?;
			require_string(record.worktree_path.as_deref(), "worktree_path")?;
			require_string(record.commit_sha.as_deref(), "commit_sha")?;
			require_string(record.transport.as_deref(), "transport")?;

			require_string(record.summary.as_deref(), "summary")
		},
		"intake" => require_string(record.summary.as_deref(), "summary"),
		"lease_acquired" => {
			require_string(record.branch.as_deref(), "branch")?;

			Ok(())
		},
		"worktree_prepared" => {
			require_string(record.branch.as_deref(), "branch")?;
			require_string(record.worktree_path.as_deref(), "worktree_path")?;

			require_string(record.commit_sha.as_deref(), "commit_sha")
		},
		"agent_started" => {
			require_string(record.branch.as_deref(), "branch")?;

			require_string(record.worktree_path.as_deref(), "worktree_path")
		},
		"progress_checkpoint" => {
			require_string(record.phase.as_deref(), "phase")?;
			require_string(record.summary.as_deref(), "summary")?;

			reject_progress_checkpoint_private_fields(record)
		},
		"pr_opened" | "pr_updated" => validate_pr_event_fields(record),
		"review_handoff" | "repair_handoff" => {
			validate_pr_event_fields(record)?;
			require_string(record.validation_result.as_deref(), "validation_result")?;
			require_string(record.summary.as_deref(), "summary")?;

			require_string(record.terminal_path.as_deref(), "terminal_path")
		},
		"review_handoff_rebind" | "review_handoff_adopt" => {
			validate_pr_event_fields(record)?;
			require_string(record.validation_result.as_deref(), "validation_result")?;
			require_string(record.summary.as_deref(), "summary")?;

			require_vec(record.evidence.as_ref(), "evidence")
		},
		"landed" => {
			validate_pr_event_fields(record)?;

			require_string(record.summary.as_deref(), "summary")
		},
		"closeout" => {
			require_string(record.pr_url.as_deref(), "pr_url")?;
			require_string(record.commit_sha.as_deref(), "commit_sha")?;

			require_string(record.summary.as_deref(), "summary")
		},
		"needs_attention" => {
			require_string(record.error_class.as_deref(), "error_class")?;
			require_string(record.next_action.as_deref(), "next_action")?;
			require_vec(record.blockers.as_ref(), "blockers")?;
			require_vec(record.evidence.as_ref(), "evidence")?;

			require_string(record.terminal_path.as_deref(), "terminal_path")
		},
		"terminal_failure" => {
			require_string(record.error_class.as_deref(), "error_class")?;
			require_string(record.next_action.as_deref(), "next_action")?;
			require_vec(record.blockers.as_ref(), "blockers")?;

			require_vec(record.evidence.as_ref(), "evidence")
		},
		"cleanup_complete" => {
			require_string(record.branch.as_deref(), "branch")?;
			require_string(record.worktree_path.as_deref(), "worktree_path")?;
			require_string(record.cleanup_status.as_deref(), "cleanup_status")?;

			require_string(record.summary.as_deref(), "summary")
		},
		other => Err(format!("Unsupported Linear execution event type `{other}`.")),
	}
}

fn reject_progress_checkpoint_private_fields(
	record: &LinearExecutionEventRecord,
) -> Result<(), String> {
	for field in [
		("focus", record.focus.is_some()),
		("next_action", record.next_action.is_some()),
		("blockers", record.blockers.is_some()),
		("evidence", record.evidence.is_some()),
		("verification", record.verification.is_some()),
		("commit_sha", record.commit_sha.is_some()),
	] {
		let (field_name, present) = field;

		if present {
			return Err(format!(
				"`progress_checkpoint` Linear events must use the public projection; `{field_name}` belongs in private execution events."
			));
		}
	}

	Ok(())
}

fn validate_pr_event_fields(record: &LinearExecutionEventRecord) -> Result<(), String> {
	require_string(record.branch.as_deref(), "branch")?;
	require_string(record.pr_url.as_deref(), "pr_url")?;
	require_string(record.pr_head_sha.as_deref(), "pr_head_sha")?;
	require_string(record.pr_base_ref.as_deref(), "pr_base_ref")?;

	require_string(record.commit_sha.as_deref(), "commit_sha")
}

fn require_string(value: Option<&str>, field: &str) -> Result<(), String> {
	if value.is_some_and(|value| !value.trim().is_empty()) {
		return Ok(());
	}

	Err(format!("`{field}` is required for this Linear execution event."))
}

fn require_vec(value: Option<&Vec<String>>, field: &str) -> Result<(), String> {
	if value.is_some() {
		return Ok(());
	}

	Err(format!("`{field}` is required for this Linear execution event."))
}

fn validate_repo_relative_path(path: &str, field_name: &str) -> Result<(), String> {
	if path.is_empty() {
		return Err(format!("`{field_name}` must not be empty."));
	}
	if path.starts_with('/') || path.starts_with("~/") || has_drive_root_prefix(path) {
		return Err(format!("`{field_name}` must be repository-relative, not `{path}`."));
	}
	if Path::new(path).components().any(|component| matches!(component, Component::ParentDir)) {
		return Err(format!("`{field_name}` must stay within the repository, not `{path}`."));
	}

	Ok(())
}

fn has_drive_root_prefix(path: &str) -> bool {
	let bytes = path.as_bytes();

	bytes.len() >= 3
		&& bytes[0].is_ascii_alphabetic()
		&& bytes[1] == b':'
		&& matches!(bytes[2], b'\\' | b'/')
}
