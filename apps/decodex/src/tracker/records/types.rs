use serde::{Deserialize, Serialize};

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

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LinearExecutionEventPublicProjection {
	pub(crate) body: String,
	pub(crate) record: LinearExecutionEventRecord,
	pub(crate) classifier_withheld_text: bool,
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
