use crate::orchestrator::{
	self, OperatorSnapshotWarningDetail, OperatorStatusSnapshot, STATUS_OPERATOR_SNAPSHOT_WARNING,
	ServiceConfig,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StatusSnapshotCacheMiss {
	pub(crate) reason: String,
	pub(crate) next_action: String,
}

pub(crate) fn add_status_snapshot_cache_miss_warning(
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

pub(crate) fn status_snapshot_cache_miss(
	reason: impl Into<String>,
	next_action: impl Into<String>,
) -> StatusSnapshotCacheMiss {
	StatusSnapshotCacheMiss { reason: reason.into(), next_action: next_action.into() }
}
