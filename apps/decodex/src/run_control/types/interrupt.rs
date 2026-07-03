use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::run_control::{
	constants::{SCHEMA_INTERRUPT_REQUEST, SCHEMA_INTERRUPT_RESPONSE},
	paths,
	types::LaneControlResponseStatus,
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
