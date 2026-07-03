use std::{
	io::{Read as _, Write as _},
	net::{SocketAddr, TcpStream},
	str,
};

use time::OffsetDateTime;

use crate::orchestrator::{
	self, DEFAULT_OPERATOR_LISTEN_ADDRESS, OPERATOR_APP_SNAPSHOT_ENDPOINT_PATH,
	OPERATOR_STATE_HEADER_TERMINATOR, OperatorSnapshotWarningDetail, OperatorStatusSnapshot,
	STATUS_OPERATOR_SNAPSHOT_CONNECT_TIMEOUT, STATUS_OPERATOR_SNAPSHOT_IO_TIMEOUT,
	STATUS_OPERATOR_SNAPSHOT_MAX_AGE, STATUS_OPERATOR_SNAPSHOT_WARNING, ServiceConfig,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StatusSnapshotCacheMiss {
	pub(crate) reason: String,
	pub(crate) next_action: String,
}

pub(crate) struct StatusSnapshotHttpResponse {
	pub(crate) body: Vec<u8>,
	pub(crate) published_at_unix_epoch: Option<i64>,
}

pub(crate) fn status_should_attempt_operator_snapshot_cache(live: bool) -> bool {
	!live
}

pub(crate) fn status_snapshot_from_operator_cache_response(
	project: &ServiceConfig,
	limit: usize,
	response: StatusSnapshotHttpResponse,
	now_unix_epoch: i64,
) -> std::result::Result<OperatorStatusSnapshot, StatusSnapshotCacheMiss> {
	let published_at_unix_epoch = response.published_at_unix_epoch.ok_or_else(|| {
		status_snapshot_cache_miss(
			"operator snapshot response omitted X-Decodex-Snapshot-Unix-Epoch",
			"wait for the operator listener to publish a fresh snapshot, or use `decodex status --live`",
		)
	})?;
	let snapshot_age_seconds = now_unix_epoch.saturating_sub(published_at_unix_epoch);

	if snapshot_age_seconds > STATUS_OPERATOR_SNAPSHOT_MAX_AGE.as_secs() as i64 {
		return Err(status_snapshot_cache_miss(
			format!(
				"operator snapshot is stale: age {snapshot_age_seconds}s exceeds {}s",
				STATUS_OPERATOR_SNAPSHOT_MAX_AGE.as_secs()
			),
			"wait for the next control-plane tick or use `decodex status --live`",
		));
	}

	let snapshot =
		serde_json::from_slice::<OperatorStatusSnapshot>(&response.body).map_err(|error| {
			status_snapshot_cache_miss(
				format!("operator snapshot JSON could not be read by this CLI: {error}"),
				"install the matching Decodex CLI/app build or use `decodex status --live`",
			)
		})?;

	project_status_snapshot_from_operator_cache(project, snapshot, limit, snapshot_age_seconds)
}

pub(super) fn status_snapshot_from_local_operator_cache(
	project: &ServiceConfig,
	limit: usize,
) -> std::result::Result<OperatorStatusSnapshot, StatusSnapshotCacheMiss> {
	let response = fetch_local_operator_snapshot_response()
		.map_err(|reason| status_snapshot_cache_miss(reason, "start or restart the local Decodex operator listener, or use `decodex status --live` for a fresh direct read"))?;

	status_snapshot_from_operator_cache_response(
		project,
		limit,
		response,
		OffsetDateTime::now_utc().unix_timestamp(),
	)
}

pub(super) fn add_status_snapshot_cache_miss_warning(
	snapshot: &mut OperatorStatusSnapshot,
	project: &ServiceConfig,
	cache_miss: StatusSnapshotCacheMiss,
) {
	orchestrator::add_operator_snapshot_warning(snapshot, STATUS_OPERATOR_SNAPSHOT_WARNING);

	snapshot.warning_details.push(OperatorSnapshotWarningDetail {
		warning: String::from(STATUS_OPERATOR_SNAPSHOT_WARNING),
		project_id: Some(project.service_id().to_owned()),
		repo_root: Some(project.repo_root().display().to_string()),
		reason: cache_miss.reason,
		next_action: Some(cache_miss.next_action),
	});
}

fn fetch_local_operator_snapshot_response()
-> std::result::Result<StatusSnapshotHttpResponse, String> {
	let address = DEFAULT_OPERATOR_LISTEN_ADDRESS
		.parse::<SocketAddr>()
		.map_err(|error| format!("default operator listener address is invalid: {error}"))?;
	let mut stream = TcpStream::connect_timeout(&address, STATUS_OPERATOR_SNAPSHOT_CONNECT_TIMEOUT)
		.map_err(|error| format!("local operator listener is unavailable at {address}: {error}"))?;

	stream
		.set_read_timeout(Some(STATUS_OPERATOR_SNAPSHOT_IO_TIMEOUT))
		.map_err(|error| format!("failed to set operator snapshot read timeout: {error}"))?;
	stream
		.set_write_timeout(Some(STATUS_OPERATOR_SNAPSHOT_IO_TIMEOUT))
		.map_err(|error| format!("failed to set operator snapshot write timeout: {error}"))?;
	stream
		.write_all(
			format!(
				"GET {OPERATOR_APP_SNAPSHOT_ENDPOINT_PATH} HTTP/1.1\r\nHost: {DEFAULT_OPERATOR_LISTEN_ADDRESS}\r\nConnection: close\r\n\r\n"
			)
			.as_bytes(),
		)
		.map_err(|error| format!("failed to request local operator snapshot: {error}"))?;

	let mut response = Vec::new();

	stream
		.read_to_end(&mut response)
		.map_err(|error| format!("failed to read local operator snapshot: {error}"))?;

	parse_operator_snapshot_http_response(&response)
}

fn parse_operator_snapshot_http_response(
	response: &[u8],
) -> std::result::Result<StatusSnapshotHttpResponse, String> {
	let header_end = response
		.windows(OPERATOR_STATE_HEADER_TERMINATOR.len())
		.position(|window| window == OPERATOR_STATE_HEADER_TERMINATOR)
		.ok_or_else(|| String::from("local operator snapshot response omitted HTTP headers"))?;
	let headers = str::from_utf8(&response[..header_end])
		.map_err(|error| format!("local operator snapshot headers were not UTF-8: {error}"))?;
	let Some(status_line) = headers.lines().next() else {
		return Err(String::from("local operator snapshot response omitted HTTP status"));
	};

	if !status_line.contains(" 200 ") {
		return Err(format!("local operator snapshot request returned `{status_line}`"));
	}

	let published_at_unix_epoch = headers.lines().find_map(|line| {
		line.strip_prefix("X-Decodex-Snapshot-Unix-Epoch: ")
			.and_then(|value| value.trim().parse::<i64>().ok())
	});
	let body = response[header_end + OPERATOR_STATE_HEADER_TERMINATOR.len()..].to_vec();

	if body.is_empty() || body.as_slice() == b"{}" {
		return Err(String::from(
			"local operator listener has not published a status snapshot yet",
		));
	}

	Ok(StatusSnapshotHttpResponse { body, published_at_unix_epoch })
}

fn project_status_snapshot_from_operator_cache(
	project: &ServiceConfig,
	mut snapshot: OperatorStatusSnapshot,
	limit: usize,
	snapshot_age_seconds: i64,
) -> std::result::Result<OperatorStatusSnapshot, StatusSnapshotCacheMiss> {
	if snapshot.run_limit < limit {
		return Err(status_snapshot_cache_miss(
			format!(
				"operator snapshot run limit {} is lower than requested status limit {limit}",
				snapshot.run_limit
			),
			"rerun with a lower `--limit`, wait for a larger published snapshot, or use `decodex status --live`",
		));
	}
	if snapshot.project_id == project.service_id() {
		mark_cached_status_snapshot(&mut snapshot, limit, snapshot_age_seconds);

		return Ok(snapshot);
	}

	let Some(project_status) =
		snapshot.projects.iter().find(|status| status.project_id == project.service_id()).cloned()
	else {
		return Err(status_snapshot_cache_miss(
			format!("operator snapshot does not include project `{}`", project.service_id()),
			"check that the local listener is serving the same registered project set, or use `decodex status --live`",
		));
	};
	let mut project_snapshot = orchestrator::empty_control_plane_snapshot(limit);

	project_snapshot.project_id = project.service_id().to_owned();
	project_snapshot.status_source = Some(String::from("operator_snapshot_cache"));
	project_snapshot.snapshot_age_seconds = Some(snapshot_age_seconds);
	project_snapshot.account_control = snapshot.account_control;
	project_snapshot.accounts = snapshot.accounts;
	project_snapshot.projects = vec![project_status];
	project_snapshot.connector_backoffs = snapshot
		.connector_backoffs
		.into_iter()
		.filter(|backoff| backoff.project_id == project.service_id())
		.collect();
	project_snapshot.current_lanes = snapshot
		.current_lanes
		.into_iter()
		.filter(|run| run.project_id == project.service_id())
		.collect();
	project_snapshot.recent_runs = snapshot
		.recent_runs
		.into_iter()
		.filter(|run| run.project_id == project.service_id())
		.collect();
	project_snapshot.history_lanes = snapshot
		.history_lanes
		.into_iter()
		.filter(|lane| lane.project_id == project.service_id())
		.collect();
	project_snapshot.queued_candidates = snapshot
		.queued_candidates
		.into_iter()
		.filter(|candidate| candidate.project_id == project.service_id())
		.collect();
	project_snapshot.worktrees = snapshot
		.worktrees
		.into_iter()
		.filter(|worktree| worktree.project_id == project.service_id())
		.collect();
	project_snapshot.post_review_lanes = snapshot
		.post_review_lanes
		.into_iter()
		.filter(|lane| lane.project_id == project.service_id())
		.collect();
	project_snapshot.warning_details = snapshot
		.warning_details
		.into_iter()
		.filter(|detail| {
			detail.project_id.as_deref().is_none()
				|| detail.project_id.as_deref() == Some(project.service_id())
		})
		.collect();
	project_snapshot.warnings = project_warnings_from_details(&project_snapshot.warning_details);

	truncate_status_snapshot_to_limit(&mut project_snapshot, limit);

	orchestrator::refresh_operator_project_summary(&mut project_snapshot, None);

	Ok(project_snapshot)
}

fn mark_cached_status_snapshot(
	snapshot: &mut OperatorStatusSnapshot,
	limit: usize,
	snapshot_age_seconds: i64,
) {
	snapshot.run_limit = limit;
	snapshot.status_source = Some(String::from("operator_snapshot_cache"));
	snapshot.snapshot_age_seconds = Some(snapshot_age_seconds);

	truncate_status_snapshot_to_limit(snapshot, limit);

	orchestrator::refresh_operator_project_summary(snapshot, None);
}

fn truncate_status_snapshot_to_limit(snapshot: &mut OperatorStatusSnapshot, limit: usize) {
	snapshot.recent_runs.truncate(limit);
	snapshot.history_lanes.truncate(limit);
}

fn project_warnings_from_details(details: &[OperatorSnapshotWarningDetail]) -> Vec<String> {
	let mut warnings = Vec::new();

	for detail in details {
		if !warnings.iter().any(|warning| warning == &detail.warning) {
			warnings.push(detail.warning.clone());
		}
	}

	warnings
}

fn status_snapshot_cache_miss(
	reason: impl Into<String>,
	next_action: impl Into<String>,
) -> StatusSnapshotCacheMiss {
	StatusSnapshotCacheMiss { reason: reason.into(), next_action: next_action.into() }
}
