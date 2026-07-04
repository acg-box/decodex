pub(super) mod client;
pub(super) mod miss;
pub(super) mod project;

pub(super) use self::miss::add_status_snapshot_cache_miss_warning;

use time::OffsetDateTime;

use crate::orchestrator::{
	OperatorStatusSnapshot, ServiceConfig, entrypoints_status_cache::miss::StatusSnapshotCacheMiss,
};

pub(crate) fn status_should_attempt_operator_snapshot_cache(live: bool) -> bool {
	!live
}

pub(super) fn status_snapshot_from_local_operator_cache(
	project: &ServiceConfig,
	limit: usize,
) -> std::result::Result<OperatorStatusSnapshot, StatusSnapshotCacheMiss> {
	let response = client::fetch_local_operator_snapshot_response()
		.map_err(|reason| miss::status_snapshot_cache_miss(reason, "start or restart the local Decodex operator listener, or use `decodex status --live` for a fresh direct read"))?;

	project::status_snapshot_from_operator_cache_response(
		project,
		limit,
		response,
		OffsetDateTime::now_utc().unix_timestamp(),
	)
}
