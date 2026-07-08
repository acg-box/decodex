use time::OffsetDateTime;

use crate::orchestrator::{
	self, OperatorConnectorBackoffStatus, OperatorStatusSnapshot, ProjectDaemonRuntime,
	TRACKER_RATE_LIMIT_WARNING, TRACKER_TRANSIENT_TIMEOUT_WARNING, TrackerConnectorBackoff,
};

pub(crate) struct ConnectorBackoffStatusParts<'a> {
	pub(crate) project_id: &'a str,
	pub(crate) connector: &'a str,
	pub(crate) sync_phase: &'a str,
	pub(crate) quota_class: &'a str,
	pub(crate) reset_unix_epoch: i64,
	pub(crate) reset_source: &'a str,
	pub(crate) warning: &'a str,
	pub(crate) next_action: &'a str,
}

impl TrackerConnectorBackoff {
	pub(crate) fn to_operator_status(
		&self,
		project_id: &str,
		now_unix_epoch: i64,
	) -> OperatorConnectorBackoffStatus {
		operator_connector_backoff_status(
			ConnectorBackoffStatusParts {
				project_id,
				connector: "linear",
				sync_phase: self.sync_phase,
				quota_class: self.quota_class,
				reset_unix_epoch: self.reset_unix_epoch,
				reset_source: self.reset_source,
				warning: self.warning,
				next_action: self.next_action,
			},
			now_unix_epoch,
		)
	}
}

pub(crate) fn warnings_include_tracker_backoff(warnings: &[&str]) -> bool {
	warnings.iter().any(|warning| tracker_backoff_warning_label(warning).is_some())
}

pub(crate) fn snapshot_warnings_include_tracker_backoff(snapshot: &OperatorStatusSnapshot) -> bool {
	snapshot.warnings.iter().any(|warning| tracker_backoff_warning_label(warning).is_some())
}

pub(crate) fn push_connector_backoff_warning(
	snapshot_warnings: &mut Vec<&'static str>,
	backoff: &OperatorConnectorBackoffStatus,
) {
	if let Some(warning) = tracker_backoff_warning_label(&backoff.warning)
		&& !snapshot_warnings.contains(&warning)
	{
		snapshot_warnings.push(warning);
	}
}

pub(crate) fn render_tracker_backoff_cli_message(
	command: &str,
	status: &OperatorConnectorBackoffStatus,
) -> String {
	format!(
		"Linear connector is in backoff for project `{}`; `{}` skipped tracker reads for `{}` until {} (retry_after_seconds={}).\n",
		status.project_id, command, status.sync_phase, status.reset_at, status.retry_after_seconds
	)
}

pub(crate) fn active_connector_backoff_statuses(
	project_id: &str,
	runtime: &ProjectDaemonRuntime,
) -> Vec<OperatorConnectorBackoffStatus> {
	let Some(backoff) = runtime.tracker_backoff.as_ref() else {
		return Vec::new();
	};
	let now_unix_epoch = OffsetDateTime::now_utc().unix_timestamp();

	vec![backoff.to_operator_status(project_id, now_unix_epoch)]
}

pub(crate) fn operator_connector_backoff_status(
	parts: ConnectorBackoffStatusParts<'_>,
	now_unix_epoch: i64,
) -> OperatorConnectorBackoffStatus {
	OperatorConnectorBackoffStatus {
		project_id: parts.project_id.to_owned(),
		connector: parts.connector.to_owned(),
		sync_phase: parts.sync_phase.to_owned(),
		quota_class: parts.quota_class.to_owned(),
		reset_at: orchestrator::format_optional_unix_timestamp(Some(parts.reset_unix_epoch))
			.unwrap_or_else(|| parts.reset_unix_epoch.to_string()),
		reset_unix_epoch: parts.reset_unix_epoch,
		reset_source: parts.reset_source.to_owned(),
		retry_after_seconds: parts.reset_unix_epoch.saturating_sub(now_unix_epoch).max(0),
		next_action: parts.next_action.to_owned(),
		warning: parts.warning.to_owned(),
	}
}

pub(crate) fn connector_backoff_next_action(warning: &str) -> &'static str {
	match warning {
		TRACKER_TRANSIENT_TIMEOUT_WARNING =>
			"Wait for the transient tracker timeout backoff; Decodex will retry tracker reads without changing lane ownership.",
		_ => "Wait for the reset window; keep monitoring local running lanes.",
	}
}

fn tracker_backoff_warning_label(warning: &str) -> Option<&'static str> {
	match warning {
		TRACKER_RATE_LIMIT_WARNING => Some(TRACKER_RATE_LIMIT_WARNING),
		TRACKER_TRANSIENT_TIMEOUT_WARNING => Some(TRACKER_TRANSIENT_TIMEOUT_WARNING),
		_ => None,
	}
}
