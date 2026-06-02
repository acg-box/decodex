use std::{
	fs,
	io::ErrorKind,
	path::{Path, PathBuf},
	process, thread,
	time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;

use crate::prelude::{self, eyre};

const RUN_CONTROL_DIR: &str = ".decodex-run-control";
const REQUEST_SUFFIX: &str = ".request.json";
const RESPONSE_SUFFIX: &str = ".response.json";
const STEER_REQUEST_SUFFIX: &str = ".steer-request.json";
const STEER_RESPONSE_SUFFIX: &str = ".steer-response.json";
const SCHEMA_INTERRUPT_REQUEST: &str = "decodex/run-control/interrupt-request/1";
const SCHEMA_INTERRUPT_RESPONSE: &str = "decodex/run-control/interrupt-response/1";
const SCHEMA_STEER_REQUEST: &str = "decodex/run-control/steer-request/1";
const SCHEMA_STEER_RESPONSE: &str = "decodex/run-control/steer-response/1";
const POLL_INTERVAL: Duration = Duration::from_millis(100);

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
			request_id: fresh_request_id(input.run_id),
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
			request_id: fresh_request_id(input.run_id),
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
			message_line_count: message_line_count(input.message),
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

pub(crate) fn write_interrupt_request(
	worktree_path: &Path,
	request: &LaneControlInterruptRequest,
) -> prelude::Result<PathBuf> {
	let path = interrupt_request_path(worktree_path, &request.run_id, &request.request_id);

	write_json_file_atomically(&path, request)?;

	Ok(path)
}

pub(crate) fn write_interrupt_response(
	worktree_path: &Path,
	response: &LaneControlInterruptResponse,
) -> prelude::Result<PathBuf> {
	let path = interrupt_response_path(worktree_path, &response.run_id, &response.request_id);

	write_json_file_atomically(&path, response)?;

	Ok(path)
}

pub(crate) fn write_steer_request(
	worktree_path: &Path,
	request: &LaneControlSteerRequest,
) -> prelude::Result<PathBuf> {
	let path = steer_request_path(worktree_path, &request.run_id, &request.request_id);

	write_json_file_atomically(&path, request)?;

	Ok(path)
}

pub(crate) fn write_steer_response(
	worktree_path: &Path,
	response: &LaneControlSteerResponse,
) -> prelude::Result<PathBuf> {
	let path = steer_response_path(worktree_path, &response.run_id, &response.request_id);

	write_json_file_atomically(&path, response)?;

	Ok(path)
}

pub(crate) fn remove_interrupt_request(path: &Path) -> prelude::Result<()> {
	match fs::remove_file(path) {
		Ok(()) => Ok(()),
		Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
		Err(error) => Err(error.into()),
	}
}

pub(crate) fn remove_steer_request(path: &Path) -> prelude::Result<()> {
	match fs::remove_file(path) {
		Ok(()) => Ok(()),
		Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
		Err(error) => Err(error.into()),
	}
}

pub(crate) fn pending_interrupt_requests(
	worktree_path: &Path,
	run_id: &str,
) -> prelude::Result<Vec<PendingLaneControlRequest>> {
	let dir = run_control_run_dir(worktree_path, run_id);
	let Ok(entries) = fs::read_dir(&dir) else {
		return Ok(Vec::new());
	};
	let mut requests = entries
		.filter_map(std::result::Result::ok)
		.map(|entry| entry.path())
		.filter(|path| file_name_ends_with(path, REQUEST_SUFFIX))
		.map(read_pending_interrupt_request)
		.collect::<prelude::Result<Vec<_>>>()?;

	requests.sort_by(|left, right| {
		left.request
			.created_at_unix_epoch
			.cmp(&right.request.created_at_unix_epoch)
			.then_with(|| left.request.request_id.cmp(&right.request.request_id))
	});

	Ok(requests)
}

pub(crate) fn pending_steer_requests(
	worktree_path: &Path,
	run_id: &str,
) -> prelude::Result<Vec<PendingLaneControlSteerRequest>> {
	let dir = run_control_run_dir(worktree_path, run_id);
	let Ok(entries) = fs::read_dir(&dir) else {
		return Ok(Vec::new());
	};
	let mut requests = entries
		.filter_map(std::result::Result::ok)
		.map(|entry| entry.path())
		.filter(|path| file_name_ends_with(path, STEER_REQUEST_SUFFIX))
		.map(read_pending_steer_request)
		.collect::<prelude::Result<Vec<_>>>()?;

	requests.sort_by(|left, right| {
		left.request
			.created_at_unix_epoch
			.cmp(&right.request.created_at_unix_epoch)
			.then_with(|| left.request.request_id.cmp(&right.request.request_id))
	});

	Ok(requests)
}

pub(crate) fn wait_for_interrupt_response(
	worktree_path: &Path,
	run_id: &str,
	request_id: &str,
	timeout: Duration,
) -> prelude::Result<Option<LaneControlInterruptResponse>> {
	let started_at = Instant::now();

	loop {
		if let Some(response) = read_interrupt_response(worktree_path, run_id, request_id)? {
			return Ok(Some(response));
		}

		if started_at.elapsed() >= timeout {
			return Ok(None);
		}

		thread::sleep(POLL_INTERVAL);
	}
}

pub(crate) fn wait_for_steer_response(
	worktree_path: &Path,
	run_id: &str,
	request_id: &str,
	timeout: Duration,
) -> prelude::Result<Option<LaneControlSteerResponse>> {
	let started_at = Instant::now();

	loop {
		if let Some(response) = read_steer_response(worktree_path, run_id, request_id)? {
			return Ok(Some(response));
		}

		if started_at.elapsed() >= timeout {
			return Ok(None);
		}

		thread::sleep(POLL_INTERVAL);
	}
}

pub(crate) fn read_interrupt_response(
	worktree_path: &Path,
	run_id: &str,
	request_id: &str,
) -> prelude::Result<Option<LaneControlInterruptResponse>> {
	let path = interrupt_response_path(worktree_path, run_id, request_id);

	match fs::read_to_string(path) {
		Ok(raw) => serde_json::from_str(&raw).map(Some).map_err(Into::into),
		Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
		Err(error) => Err(error.into()),
	}
}

pub(crate) fn read_steer_response(
	worktree_path: &Path,
	run_id: &str,
	request_id: &str,
) -> prelude::Result<Option<LaneControlSteerResponse>> {
	let path = steer_response_path(worktree_path, run_id, request_id);

	match fs::read_to_string(path) {
		Ok(raw) => serde_json::from_str(&raw).map(Some).map_err(Into::into),
		Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
		Err(error) => Err(error.into()),
	}
}

pub(crate) fn protocol_response_summary(value: &Value) -> String {
	match value {
		Value::Null => String::from("null"),
		Value::Bool(_) => String::from("boolean"),
		Value::Number(_) => String::from("number"),
		Value::String(_) => String::from("string"),
		Value::Array(values) => format!("array(len={})", values.len()),
		Value::Object(entries) => {
			let mut keys = entries.keys().map(String::as_str).collect::<Vec<_>>();

			keys.sort_unstable();

			format!("object(keys={})", keys.join(","))
		},
	}
}

fn read_pending_interrupt_request(path: PathBuf) -> prelude::Result<PendingLaneControlRequest> {
	let raw = fs::read_to_string(&path)?;
	let request: LaneControlInterruptRequest = serde_json::from_str(&raw)?;

	if request.schema != SCHEMA_INTERRUPT_REQUEST {
		eyre::bail!(
			"Unsupported lane-control request schema `{}` in `{}`.",
			request.schema,
			path.display()
		);
	}

	Ok(PendingLaneControlRequest { path, request })
}

fn read_pending_steer_request(path: PathBuf) -> prelude::Result<PendingLaneControlSteerRequest> {
	let raw = fs::read_to_string(&path)?;
	let request: LaneControlSteerRequest = serde_json::from_str(&raw)?;

	if request.schema != SCHEMA_STEER_REQUEST {
		eyre::bail!(
			"Unsupported lane-control steer request schema `{}` in `{}`.",
			request.schema,
			path.display()
		);
	}

	Ok(PendingLaneControlSteerRequest { path, request })
}

fn interrupt_request_path(worktree_path: &Path, run_id: &str, request_id: &str) -> PathBuf {
	run_control_run_dir(worktree_path, run_id).join(format!(
		"{}{}",
		sanitize_path_component(request_id),
		REQUEST_SUFFIX
	))
}

fn interrupt_response_path(worktree_path: &Path, run_id: &str, request_id: &str) -> PathBuf {
	run_control_run_dir(worktree_path, run_id).join(format!(
		"{}{}",
		sanitize_path_component(request_id),
		RESPONSE_SUFFIX
	))
}

fn steer_request_path(worktree_path: &Path, run_id: &str, request_id: &str) -> PathBuf {
	run_control_run_dir(worktree_path, run_id).join(format!(
		"{}{}",
		sanitize_path_component(request_id),
		STEER_REQUEST_SUFFIX
	))
}

fn steer_response_path(worktree_path: &Path, run_id: &str, request_id: &str) -> PathBuf {
	run_control_run_dir(worktree_path, run_id).join(format!(
		"{}{}",
		sanitize_path_component(request_id),
		STEER_RESPONSE_SUFFIX
	))
}

fn run_control_run_dir(worktree_path: &Path, run_id: &str) -> PathBuf {
	worktree_path.join(RUN_CONTROL_DIR).join(sanitize_path_component(run_id))
}

fn write_json_file_atomically<T>(path: &Path, value: &T) -> prelude::Result<()>
where
	T: Serialize,
{
	let parent = path
		.parent()
		.ok_or_else(|| eyre::eyre!("Lane-control file `{}` has no parent.", path.display()))?;
	let temp_path = path.with_extension("tmp");
	let data = serde_json::to_vec_pretty(value)?;

	fs::create_dir_all(parent)?;
	fs::write(&temp_path, data)?;
	fs::rename(&temp_path, path)?;

	Ok(())
}

fn fresh_request_id(run_id: &str) -> String {
	let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or(Duration::ZERO).as_nanos();

	format!("{}-{}-{now}", sanitize_path_component(run_id), process::id())
}

fn file_name_ends_with(path: &Path, suffix: &str) -> bool {
	path.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.ends_with(suffix))
}

fn message_line_count(message: &str) -> usize {
	message.lines().count().max(usize::from(!message.is_empty()))
}

fn sanitize_path_component(value: &str) -> String {
	let sanitized = value
		.chars()
		.map(|character| match character {
			'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' => character,
			_ => '-',
		})
		.collect::<String>();

	if sanitized.is_empty() { String::from("lane-control") } else { sanitized }
}
