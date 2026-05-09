use std::path::{Component, Path};

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Error;

use crate::tracker::TrackerComment;

#[cfg(test)]
pub(crate) const REVIEW_HANDOFF_RECORD_TYPE: &str = "review-handoff-record/1";
#[cfg(test)]
pub(crate) const CLOSEOUT_RECORD_TYPE: &str = "closeout-record/1";
pub(crate) const LINEAR_EXECUTION_EVENT_RECORD_TYPE: &str = "decodex.linear_execution_event";
pub(crate) const LINEAR_EXECUTION_EVENT_RECORD_VERSION: i64 = 1;

#[cfg(test)]
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct ReviewHandoffRecord {
	#[serde(rename = "type")]
	pub(crate) record_type: String,
	pub(crate) completed_at: String,
	pub(crate) run_id: String,
	pub(crate) attempt_number: i64,
	pub(crate) branch_name: String,
	pub(crate) pr_url: String,
	pub(crate) target_base_ref_name: String,
	pub(crate) pr_head_ref_name: String,
	pub(crate) pr_head_oid: String,
	pub(crate) summary: String,
}

#[cfg(test)]
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct CloseoutRecord {
	#[serde(rename = "type")]
	pub(crate) record_type: String,
	pub(crate) completed_at: String,
	pub(crate) run_id: String,
	pub(crate) attempt_number: i64,
	pub(crate) branch_name: String,
	pub(crate) pr_url: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LinearExecutionEventRecord {
	pub(crate) record_type: String,
	pub(crate) record_version: i64,
	pub(crate) event_type: String,
	pub(crate) event_timestamp: String,
	pub(crate) idempotency_key: String,
	pub(crate) service_id: String,
	pub(crate) issue_id: String,
	pub(crate) issue_identifier: String,
	pub(crate) run_id: String,
	pub(crate) attempt_number: i64,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) branch: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) worktree_path: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) commit_sha: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) pr_url: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) pr_head_sha: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) pr_base_ref: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) summary: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) validation_result: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) phase: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) focus: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) next_action: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) blockers: Option<Vec<String>>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) evidence: Option<Vec<String>>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) verification: Option<Vec<String>>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) error_class: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) terminal_path: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) cleanup_status: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) transport: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) target_state: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) failed_command: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) raw_error: Option<String>,
}
impl LinearExecutionEventRecord {
	pub(crate) fn new(
		identity: LinearExecutionEventIdentity<'_>,
		event_type: &str,
		event_timestamp: String,
		stable_anchor: &str,
	) -> Self {
		Self {
			record_type: String::from(LINEAR_EXECUTION_EVENT_RECORD_TYPE),
			record_version: LINEAR_EXECUTION_EVENT_RECORD_VERSION,
			event_type: event_type.to_owned(),
			event_timestamp,
			idempotency_key: linear_execution_idempotency_key(
				identity.service_id,
				identity.issue_identifier,
				identity.run_id,
				identity.attempt_number,
				event_type,
				stable_anchor,
			),
			service_id: identity.service_id.to_owned(),
			issue_id: identity.issue_id.to_owned(),
			issue_identifier: identity.issue_identifier.to_owned(),
			run_id: identity.run_id.to_owned(),
			attempt_number: identity.attempt_number,
			branch: None,
			worktree_path: None,
			commit_sha: None,
			pr_url: None,
			pr_head_sha: None,
			pr_base_ref: None,
			summary: None,
			validation_result: None,
			phase: None,
			focus: None,
			next_action: None,
			blockers: None,
			evidence: None,
			verification: None,
			error_class: None,
			terminal_path: None,
			cleanup_status: None,
			transport: None,
			target_state: None,
			failed_command: None,
			raw_error: None,
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LinearExecutionEventIdentity<'a> {
	pub(crate) service_id: &'a str,
	pub(crate) issue_id: &'a str,
	pub(crate) issue_identifier: &'a str,
	pub(crate) run_id: &'a str,
	pub(crate) attempt_number: i64,
}

pub(crate) fn format_structured_comment<T>(record: &T) -> std::result::Result<String, Error>
where
	T: Serialize,
{
	let payload = serde_json::to_string_pretty(record)?;

	Ok(format!("```json\n{payload}\n```"))
}

pub(crate) fn append_structured_comment_record<T>(
	body: &str,
	record: &T,
) -> std::result::Result<String, Error>
where
	T: Serialize,
{
	let payload = format_structured_comment(record)?;

	if body.trim().is_empty() {
		return Ok(payload);
	}

	Ok(format!("{body}\n\n{payload}"))
}

pub(crate) fn stable_event_anchor(parts: &[&str]) -> String {
	let mut hash = 0xcbf29ce484222325_u64;

	for part in parts {
		for byte in part.as_bytes() {
			hash ^= u64::from(*byte);
			hash = hash.wrapping_mul(0x100000001b3);
		}

		hash ^= 0xff;
		hash = hash.wrapping_mul(0x100000001b3);
	}

	format!("{hash:016x}")
}

pub(crate) fn validate_linear_execution_event_record(
	record: &LinearExecutionEventRecord,
) -> Result<(), String> {
	validate_linear_execution_event_envelope(record)?;
	validate_linear_execution_event_fields(record)?;

	if let Some(worktree_path) = record.worktree_path.as_deref() {
		validate_repo_relative_path(worktree_path, "worktree_path")?;
	}

	Ok(())
}

pub(crate) fn has_linear_execution_event_record(
	comments: &[TrackerComment],
	service_id: &str,
	issue_id: &str,
	idempotency_key: &str,
) -> bool {
	comments.iter().filter_map(|comment| parse_linear_execution_event_record(&comment.body)).any(
		|record| {
			record.service_id == service_id
				&& record.issue_id == issue_id
				&& record.idempotency_key == idempotency_key
		},
	)
}

pub(crate) fn parse_linear_execution_event_record(
	body: &str,
) -> Option<LinearExecutionEventRecord> {
	parse_structured_comment::<LinearExecutionEventRecord>(body)
		.filter(|record| validate_linear_execution_event_record(record).is_ok())
}

fn parse_structured_comment<T>(body: &str) -> Option<T>
where
	T: DeserializeOwned,
{
	extract_structured_json_blocks(body)
		.into_iter()
		.rev()
		.find_map(|payload| serde_json::from_str::<T>(payload).ok())
		.or_else(|| serde_json::from_str::<T>(body.trim()).ok())
}

fn extract_structured_json_blocks(body: &str) -> Vec<&str> {
	body.match_indices("```json")
		.filter_map(|(start, _)| {
			let fenced = &body[start + "```json".len()..];
			let fenced = fenced.strip_prefix("\r\n").or_else(|| fenced.strip_prefix('\n'))?;
			let end = fenced.find("\n```").or_else(|| fenced.find("\r\n```"))?;

			Some(fenced[..end].trim())
		})
		.collect()
}

fn linear_execution_idempotency_key(
	service_id: &str,
	issue_identifier: &str,
	run_id: &str,
	attempt_number: i64,
	event_type: &str,
	stable_anchor: &str,
) -> String {
	format!(
		"{service_id}:{issue_identifier}:{run_id}:{attempt_number}:{event_type}:{stable_anchor}"
	)
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
			require_string(record.focus.as_deref(), "focus")?;
			require_string(record.next_action.as_deref(), "next_action")?;
			require_vec(record.blockers.as_ref(), "blockers")?;

			require_vec(record.evidence.as_ref(), "evidence")
		},
		"pr_opened" | "pr_updated" => validate_pr_event_fields(record),
		"review_handoff" | "repair_handoff" => {
			validate_pr_event_fields(record)?;
			require_string(record.validation_result.as_deref(), "validation_result")?;
			require_string(record.summary.as_deref(), "summary")?;

			require_string(record.terminal_path.as_deref(), "terminal_path")
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
