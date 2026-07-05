use crate::tracker::records::LinearExecutionEventRecord;

pub(in crate::tracker::records::validation) fn validate_linear_execution_event_fields(
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
