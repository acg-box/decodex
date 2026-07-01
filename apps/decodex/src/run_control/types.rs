use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::run_control::{
	constants::{
		SCHEMA_INTERRUPT_REQUEST, SCHEMA_INTERRUPT_RESPONSE, SCHEMA_STEER_REQUEST,
		SCHEMA_STEER_RESPONSE,
	},
	paths,
};

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LaneControlInterruptRequest {
	pub(crate) schema: String,
	pub(crate) request_id: String,
	pub(crate) project_id: String,
	pub(crate) issue_id: String,
	pub(crate) run_id: String,
	pub(crate) attempt_number: i64,
	pub(crate) thread_id: String,
	pub(crate) turn_id: String,
	pub(crate) source: String,
	pub(crate) reason: Option<String>,
	pub(crate) created_at_unix_epoch: i64,
}
impl LaneControlInterruptRequest {
	pub(crate) fn new(input: LaneControlInterruptRequestInput<'_>) -> Self {
		Self {
			schema: String::from(SCHEMA_INTERRUPT_REQUEST),
			request_id: paths::fresh_request_id(input.run_id),
			project_id: input.project_id.to_owned(),
			issue_id: input.issue_id.to_owned(),
			run_id: input.run_id.to_owned(),
			attempt_number: input.attempt_number,
			thread_id: input.thread_id.to_owned(),
			turn_id: input.turn_id.to_owned(),
			source: input.source.to_owned(),
			reason: input.reason.map(str::to_owned),
			created_at_unix_epoch: OffsetDateTime::now_utc().unix_timestamp(),
		}
	}
}

pub(crate) struct LaneControlInterruptRequestInput<'a> {
	pub(crate) project_id: &'a str,
	pub(crate) issue_id: &'a str,
	pub(crate) run_id: &'a str,
	pub(crate) attempt_number: i64,
	pub(crate) thread_id: &'a str,
	pub(crate) turn_id: &'a str,
	pub(crate) source: &'a str,
	pub(crate) reason: Option<&'a str>,
}

#[derive(Clone, Debug)]
pub(crate) struct PendingLaneControlRequest {
	pub(crate) path: PathBuf,
	pub(crate) request: LaneControlInterruptRequest,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LaneControlSteerRequest {
	pub(crate) schema: String,
	pub(crate) request_id: String,
	pub(crate) audit_record_id: i64,
	pub(crate) project_id: String,
	pub(crate) issue_id: String,
	pub(crate) run_id: String,
	pub(crate) attempt_number: i64,
	pub(crate) thread_id: String,
	pub(crate) expected_turn_id: String,
	pub(crate) source: String,
	pub(crate) message: String,
	pub(crate) message_byte_count: usize,
	pub(crate) message_line_count: usize,
	pub(crate) created_at_unix_epoch: i64,
}
impl LaneControlSteerRequest {
	pub(crate) fn new(input: LaneControlSteerRequestInput<'_>) -> Self {
		Self {
			schema: String::from(SCHEMA_STEER_REQUEST),
			request_id: paths::fresh_request_id(input.run_id),
			audit_record_id: input.audit_record_id,
			project_id: input.project_id.to_owned(),
			issue_id: input.issue_id.to_owned(),
			run_id: input.run_id.to_owned(),
			attempt_number: input.attempt_number,
			thread_id: input.thread_id.to_owned(),
			expected_turn_id: input.expected_turn_id.to_owned(),
			source: input.source.to_owned(),
			message: input.message.to_owned(),
			message_byte_count: input.message.len(),
			message_line_count: paths::message_line_count(input.message),
			created_at_unix_epoch: OffsetDateTime::now_utc().unix_timestamp(),
		}
	}
}

pub(crate) struct LaneControlSteerRequestInput<'a> {
	pub(crate) audit_record_id: i64,
	pub(crate) project_id: &'a str,
	pub(crate) issue_id: &'a str,
	pub(crate) run_id: &'a str,
	pub(crate) attempt_number: i64,
	pub(crate) thread_id: &'a str,
	pub(crate) expected_turn_id: &'a str,
	pub(crate) source: &'a str,
	pub(crate) message: &'a str,
}

#[derive(Clone, Debug)]
pub(crate) struct PendingLaneControlSteerRequest {
	pub(crate) path: PathBuf,
	pub(crate) request: LaneControlSteerRequest,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LaneControlResponseStatus {
	SoftDelivered,
	SoftFailed,
	Rejected,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LaneControlSteerResponseStatus {
	Delivered,
	Failed,
	Rejected,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LaneControlInterruptResponse {
	pub(crate) schema: String,
	pub(crate) request_id: String,
	pub(crate) project_id: String,
	pub(crate) issue_id: String,
	pub(crate) run_id: String,
	pub(crate) attempt_number: i64,
	pub(crate) thread_id: String,
	pub(crate) turn_id: String,
	pub(crate) status: LaneControlResponseStatus,
	pub(crate) classification: String,
	pub(crate) method: String,
	pub(crate) message: String,
	pub(crate) error_class: Option<String>,
	pub(crate) protocol_summary: Option<String>,
	pub(crate) recorded_at_unix_epoch: i64,
}
impl LaneControlInterruptResponse {
	pub(crate) fn delivered(
		request: &LaneControlInterruptRequest,
		protocol_summary: String,
	) -> Self {
		Self::from_request(
			request,
			LaneControlResponseStatus::SoftDelivered,
			"graceful_stop_requested",
			"turn/interrupt accepted by app-server.",
			None,
			Some(protocol_summary),
		)
	}

	pub(crate) fn failed(
		request: &LaneControlInterruptRequest,
		error_class: &str,
		message: String,
	) -> Self {
		Self::from_request(
			request,
			LaneControlResponseStatus::SoftFailed,
			"soft_interrupt_failed",
			message,
			Some(error_class.to_owned()),
			None,
		)
	}

	pub(crate) fn rejected(
		request: &LaneControlInterruptRequest,
		error_class: &str,
		message: String,
	) -> Self {
		Self::from_request(
			request,
			LaneControlResponseStatus::Rejected,
			"control_request_rejected",
			message,
			Some(error_class.to_owned()),
			None,
		)
	}

	fn from_request(
		request: &LaneControlInterruptRequest,
		status: LaneControlResponseStatus,
		classification: &str,
		message: impl Into<String>,
		error_class: Option<String>,
		protocol_summary: Option<String>,
	) -> Self {
		Self {
			schema: String::from(SCHEMA_INTERRUPT_RESPONSE),
			request_id: request.request_id.clone(),
			project_id: request.project_id.clone(),
			issue_id: request.issue_id.clone(),
			run_id: request.run_id.clone(),
			attempt_number: request.attempt_number,
			thread_id: request.thread_id.clone(),
			turn_id: request.turn_id.clone(),
			status,
			classification: classification.to_owned(),
			method: String::from("turn/interrupt"),
			message: message.into(),
			error_class,
			protocol_summary,
			recorded_at_unix_epoch: OffsetDateTime::now_utc().unix_timestamp(),
		}
	}
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LaneControlSteerResponse {
	pub(crate) schema: String,
	pub(crate) request_id: String,
	pub(crate) project_id: String,
	pub(crate) issue_id: String,
	pub(crate) run_id: String,
	pub(crate) attempt_number: i64,
	pub(crate) thread_id: String,
	pub(crate) expected_turn_id: String,
	pub(crate) current_turn_id: Option<String>,
	pub(crate) response_turn_id: Option<String>,
	pub(crate) status: LaneControlSteerResponseStatus,
	pub(crate) classification: String,
	pub(crate) method: String,
	pub(crate) message: String,
	pub(crate) error_class: Option<String>,
	pub(crate) recorded_at_unix_epoch: i64,
}
impl LaneControlSteerResponse {
	pub(crate) fn delivered(
		request: &LaneControlSteerRequest,
		current_turn_id: &str,
		response_turn_id: &str,
	) -> Self {
		Self::from_request(
			request,
			LaneControlSteerResponseStatus::Delivered,
			"turn_steer_delivered",
			"turn/steer accepted by app-server.",
			None,
			Some(current_turn_id.to_owned()),
			Some(response_turn_id.to_owned()),
		)
	}

	pub(crate) fn failed(
		request: &LaneControlSteerRequest,
		current_turn_id: &str,
		error_class: &str,
		message: String,
	) -> Self {
		Self::from_request(
			request,
			LaneControlSteerResponseStatus::Failed,
			error_class,
			message,
			Some(error_class.to_owned()),
			Some(current_turn_id.to_owned()),
			None,
		)
	}

	pub(crate) fn rejected(
		request: &LaneControlSteerRequest,
		current_turn_id: &str,
		error_class: &str,
		message: String,
	) -> Self {
		Self::from_request(
			request,
			LaneControlSteerResponseStatus::Rejected,
			"control_request_rejected",
			message,
			Some(error_class.to_owned()),
			Some(current_turn_id.to_owned()),
			None,
		)
	}

	fn from_request(
		request: &LaneControlSteerRequest,
		status: LaneControlSteerResponseStatus,
		classification: &str,
		message: impl Into<String>,
		error_class: Option<String>,
		current_turn_id: Option<String>,
		response_turn_id: Option<String>,
	) -> Self {
		Self {
			schema: String::from(SCHEMA_STEER_RESPONSE),
			request_id: request.request_id.clone(),
			project_id: request.project_id.clone(),
			issue_id: request.issue_id.clone(),
			run_id: request.run_id.clone(),
			attempt_number: request.attempt_number,
			thread_id: request.thread_id.clone(),
			expected_turn_id: request.expected_turn_id.clone(),
			current_turn_id,
			response_turn_id,
			status,
			classification: classification.to_owned(),
			method: String::from("turn/steer"),
			message: message.into(),
			error_class,
			recorded_at_unix_epoch: OffsetDateTime::now_utc().unix_timestamp(),
		}
	}
}
