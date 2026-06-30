use std::{
	path::Path,
	time::{Duration, Instant},
};

use color_eyre::Report;
use time::OffsetDateTime;

use super::{
	AccountActivityMode, GhPullRequestReviewStateInspector, OperatorConnectorBackoffStatus,
	OperatorStatusSnapshot, ProjectDaemonRuntime, ServiceConfig, StateStore,
	TRACKER_RATE_LIMIT_BACKOFF_SECS, TRACKER_RATE_LIMIT_WARNING,
	TRACKER_TRANSIENT_TIMEOUT_BACKOFF_SECS, TRACKER_TRANSIENT_TIMEOUT_WARNING,
	TrackerConnectorBackoff, add_operator_snapshot_warning, apply_terminal_history_ledger_outcomes,
	build_degraded_post_review_lane_statuses, build_operator_status_snapshot_with_account_mode,
	format_optional_unix_timestamp, hydrate_history_lanes_from_local_ledger,
	refresh_operator_project_summary,
};
use crate::{
	prelude::Result,
	state::{ConnectorBackoff, ConnectorBackoffInput},
};

struct ConnectorBackoffStatusParts<'a> {
	project_id: &'a str,
	connector: &'a str,
	sync_phase: &'a str,
	quota_class: &'a str,
	reset_unix_epoch: i64,
	reset_source: &'a str,
	warning: &'a str,
	next_action: &'a str,
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

fn operator_connector_backoff_status(
	parts: ConnectorBackoffStatusParts<'_>,
	now_unix_epoch: i64,
) -> OperatorConnectorBackoffStatus {
	OperatorConnectorBackoffStatus {
		project_id: parts.project_id.to_owned(),
		connector: parts.connector.to_owned(),
		sync_phase: parts.sync_phase.to_owned(),
		quota_class: parts.quota_class.to_owned(),
		reset_at: format_optional_unix_timestamp(Some(parts.reset_unix_epoch))
			.unwrap_or_else(|| parts.reset_unix_epoch.to_string()),
		reset_unix_epoch: parts.reset_unix_epoch,
		reset_source: parts.reset_source.to_owned(),
		retry_after_seconds: parts.reset_unix_epoch.saturating_sub(now_unix_epoch).max(0),
		next_action: parts.next_action.to_owned(),
		warning: parts.warning.to_owned(),
	}
}

fn connector_backoff_record_to_operator_status(
	backoff: &ConnectorBackoff,
	now_unix_epoch: i64,
) -> OperatorConnectorBackoffStatus {
	operator_connector_backoff_status(
		ConnectorBackoffStatusParts {
			project_id: backoff.project_id(),
			connector: backoff.connector(),
			sync_phase: backoff.sync_phase(),
			quota_class: backoff.quota_class(),
			reset_unix_epoch: backoff.reset_unix_epoch(),
			reset_source: backoff.reset_source(),
			warning: backoff.warning(),
			next_action: connector_backoff_next_action(backoff.warning()),
		},
		now_unix_epoch,
	)
}

fn connector_backoff_next_action(warning: &str) -> &'static str {
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

pub(crate) fn active_stored_tracker_backoff_status(
	state_store: &StateStore,
	project_id: &str,
) -> Result<Option<OperatorConnectorBackoffStatus>> {
	let Some(backoff) = state_store.connector_backoff(project_id, "linear")? else {
		return Ok(None);
	};
	let now_unix_epoch = OffsetDateTime::now_utc().unix_timestamp();

	if backoff.reset_unix_epoch() <= now_unix_epoch {
		state_store.clear_connector_backoff(project_id, "linear")?;

		return Ok(None);
	}

	Ok(Some(connector_backoff_record_to_operator_status(&backoff, now_unix_epoch)))
}

pub(crate) fn active_stored_tracker_backoff_status_best_effort(
	state_store: &StateStore,
	project_id: &str,
) -> Option<OperatorConnectorBackoffStatus> {
	match active_stored_tracker_backoff_status(state_store, project_id) {
		Ok(status) => status,
		Err(error) => {
			let _ = error;

			tracing::warn!(
				project_id = project_id,
				"Failed to read persisted tracker backoff; sensitive runtime details were withheld."
			);

			None
		},
	}
}

pub(crate) fn persist_tracker_backoff_state(
	state_store: &StateStore,
	project_id: &str,
	backoff: &TrackerConnectorBackoff,
) {
	if let Err(error) = state_store.upsert_connector_backoff(ConnectorBackoffInput {
		project_id,
		connector: "linear",
		sync_phase: backoff.sync_phase,
		quota_class: backoff.quota_class,
		reset_unix_epoch: backoff.reset_unix_epoch,
		reset_source: backoff.reset_source,
		warning: backoff.warning,
	}) {
		let _ = error;

		tracing::warn!(
			project_id = project_id,
			"Failed to persist tracker backoff; sensitive runtime details were withheld."
		);
	}
}

pub(crate) fn clear_tracker_backoff_state_best_effort(state_store: &StateStore, project_id: &str) {
	if let Err(error) = state_store.clear_connector_backoff(project_id, "linear") {
		let _ = error;

		tracing::warn!(
			project_id = project_id,
			"Failed to clear persisted tracker backoff; sensitive runtime details were withheld."
		);
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

pub(crate) fn build_operator_status_snapshot_for_tracker_backoff(
	project: &ServiceConfig,
	state_store: &StateStore,
	limit: usize,
	status: &OperatorConnectorBackoffStatus,
) -> Result<OperatorStatusSnapshot> {
	let review_state_inspector = GhPullRequestReviewStateInspector {
		github_token_env_var: Some(project.github().token_env_var().to_owned()),
		github_command_path: project.github().command_path().map(Path::to_path_buf),
	};
	let mut snapshot = build_operator_status_snapshot_with_account_mode(
		project,
		state_store,
		limit,
		AccountActivityMode::Snapshot,
	)?;

	hydrate_history_lanes_from_local_ledger(project, state_store, &mut snapshot)?;

	snapshot.post_review_lanes =
		build_degraded_post_review_lane_statuses(project, state_store, &review_state_inspector)?;

	add_operator_snapshot_warning(&mut snapshot, &status.warning);

	snapshot.connector_backoffs.push(status.clone());

	add_operator_snapshot_warning(&mut snapshot, "external_observer_status_skipped");
	apply_terminal_history_ledger_outcomes(&mut snapshot);
	refresh_operator_project_summary(&mut snapshot, None);

	Ok(snapshot)
}

pub(crate) fn tracker_connector_backoff(
	error: &Report,
	now: Instant,
	sync_phase: &'static str,
) -> Option<TrackerConnectorBackoff> {
	let message = format!("{error:#}");

	if message.contains("Linear connector is rate limited") {
		return tracker_connector_backoff_from_message(&message, now, sync_phase);
	}
	if message.contains("Linear connector timed out") {
		return tracker_timeout_backoff(now, sync_phase);
	}

	None
}

fn tracker_connector_backoff_from_message(
	message: &str,
	now: Instant,
	sync_phase: &'static str,
) -> Option<TrackerConnectorBackoff> {
	let now_unix_epoch = OffsetDateTime::now_utc().unix_timestamp();
	let fallback_reset_unix_epoch =
		now_unix_epoch.saturating_add(TRACKER_RATE_LIMIT_BACKOFF_SECS as i64);
	let (reset_unix_epoch, reset_source) = match parse_linear_rate_limit_reset_unix_epoch(message) {
		Some(reset_unix_epoch) if reset_unix_epoch > now_unix_epoch => (reset_unix_epoch, "linear"),
		_ => (fallback_reset_unix_epoch, "local_default"),
	};
	let retry_after_seconds = reset_unix_epoch - now_unix_epoch;
	let retry_after_seconds = u64::try_from(retry_after_seconds).ok()?;

	Some(TrackerConnectorBackoff {
		until: now + Duration::from_secs(retry_after_seconds),
		quota_class: "linear_graphql_rate_limit",
		reset_unix_epoch,
		reset_source,
		sync_phase,
		warning: TRACKER_RATE_LIMIT_WARNING,
		next_action: "Wait for the reset window; keep monitoring local running lanes.",
	})
}

fn tracker_timeout_backoff(
	now: Instant,
	sync_phase: &'static str,
) -> Option<TrackerConnectorBackoff> {
	let now_unix_epoch = OffsetDateTime::now_utc().unix_timestamp();
	let reset_unix_epoch =
		now_unix_epoch.saturating_add(TRACKER_TRANSIENT_TIMEOUT_BACKOFF_SECS as i64);

	Some(TrackerConnectorBackoff {
		until: now + Duration::from_secs(TRACKER_TRANSIENT_TIMEOUT_BACKOFF_SECS),
		quota_class: "linear_graphql_timeout",
		reset_unix_epoch,
		reset_source: "local_transient_timeout",
		sync_phase,
		warning: TRACKER_TRANSIENT_TIMEOUT_WARNING,
		next_action: "Wait for the transient tracker timeout backoff; Decodex will retry tracker reads without changing lane ownership.",
	})
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

fn parse_linear_rate_limit_reset_unix_epoch(message: &str) -> Option<i64> {
	let reset = message.split("rate limited until `").nth(1)?.split('`').next()?;

	reset.parse().ok()
}
