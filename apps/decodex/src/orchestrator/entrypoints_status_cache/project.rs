use crate::orchestrator::{
	self, OperatorSnapshotWarningDetail, OperatorStatusSnapshot, STATUS_OPERATOR_SNAPSHOT_MAX_AGE,
	ServiceConfig,
	entrypoints_status_cache::{
		client::StatusSnapshotHttpResponse,
		miss::{self, StatusSnapshotCacheMiss},
	},
};

pub(crate) fn status_snapshot_from_operator_cache_response(
	project: &ServiceConfig,
	limit: usize,
	response: StatusSnapshotHttpResponse,
	now_unix_epoch: i64,
) -> std::result::Result<OperatorStatusSnapshot, StatusSnapshotCacheMiss> {
	let published_at_unix_epoch = response.published_at_unix_epoch.ok_or_else(|| {
		miss::status_snapshot_cache_miss(
			"operator snapshot response omitted X-Decodex-Snapshot-Unix-Epoch",
			"wait for the operator listener to publish a fresh snapshot, or use `decodex status --live`",
		)
	})?;
	let snapshot_age_seconds = now_unix_epoch.saturating_sub(published_at_unix_epoch);

	if snapshot_age_seconds > STATUS_OPERATOR_SNAPSHOT_MAX_AGE.as_secs() as i64 {
		return Err(miss::status_snapshot_cache_miss(
			format!(
				"operator snapshot is stale: age {snapshot_age_seconds}s exceeds {}s",
				STATUS_OPERATOR_SNAPSHOT_MAX_AGE.as_secs()
			),
			"wait for the next control-plane tick or use `decodex status --live`",
		));
	}

	let snapshot =
		serde_json::from_slice::<OperatorStatusSnapshot>(&response.body).map_err(|error| {
			miss::status_snapshot_cache_miss(
				format!("operator snapshot JSON could not be read by this CLI: {error}"),
				"install the matching Decodex CLI/app build or use `decodex status --live`",
			)
		})?;

	project_status_snapshot_from_operator_cache(project, snapshot, limit, snapshot_age_seconds)
}

fn project_status_snapshot_from_operator_cache(
	project: &ServiceConfig,
	mut snapshot: OperatorStatusSnapshot,
	limit: usize,
	snapshot_age_seconds: i64,
) -> std::result::Result<OperatorStatusSnapshot, StatusSnapshotCacheMiss> {
	if snapshot.run_limit < limit {
		return Err(miss::status_snapshot_cache_miss(
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
		return Err(miss::status_snapshot_cache_miss(
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
