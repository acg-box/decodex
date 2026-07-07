use color_eyre::Report;
use time::OffsetDateTime;

use crate::{
	prelude::Result,
	recovery::context::{LINEAR_RATE_LIMIT_BACKOFF_WARNING, RecoveryContext},
	state::ConnectorBackoffInput,
};

const LINEAR_RATE_LIMIT_BACKOFF_SECS: i64 = 15 * 60;
const LINEAR_TRANSIENT_TIMEOUT_BACKOFF_WARNING: &str = "tracker_transient_timeout";
const LINEAR_TRANSIENT_TIMEOUT_BACKOFF_SECS: i64 = 60;

struct RecoveryTrackerBackoff<'a> {
	quota_class: &'a str,
	reset_unix_epoch: i64,
	reset_source: &'a str,
	warning: &'a str,
}

pub(in crate::recovery) fn active_recovery_tracker_backoff_message(
	context: &RecoveryContext,
) -> Result<Option<String>> {
	let Some(backoff) =
		context.state_store.connector_backoff(context.config.service_id(), "linear")?
	else {
		return Ok(None);
	};
	let now_unix_epoch = OffsetDateTime::now_utc().unix_timestamp();

	if backoff.reset_unix_epoch() <= now_unix_epoch {
		if context.runtime_mutation_policy.allows_runtime_writes() {
			context.state_store.clear_connector_backoff(context.config.service_id(), "linear")?;
		}

		return Ok(None);
	}

	Ok(Some(recovery_tracker_backoff_message(
		context.config.service_id(),
		backoff.sync_phase(),
		backoff.reset_unix_epoch(),
		backoff.reset_unix_epoch().saturating_sub(now_unix_epoch),
	)))
}

pub(in crate::recovery) fn remember_recovery_tracker_backoff_message(
	context: &RecoveryContext,
	error: &Report,
	sync_phase: &str,
) -> Option<String> {
	let message = format!("{error:#}");
	let now_unix_epoch = OffsetDateTime::now_utc().unix_timestamp();
	let backoff = recovery_tracker_backoff_from_message(&message, now_unix_epoch)?;

	if !context.runtime_mutation_policy.allows_runtime_writes() {
		return Some(recovery_tracker_backoff_message(
			context.config.service_id(),
			sync_phase,
			backoff.reset_unix_epoch,
			backoff.reset_unix_epoch.saturating_sub(now_unix_epoch),
		));
	}

	if let Err(store_error) = context.state_store.upsert_connector_backoff(ConnectorBackoffInput {
		project_id: context.config.service_id(),
		connector: "linear",
		sync_phase,
		quota_class: backoff.quota_class,
		reset_unix_epoch: backoff.reset_unix_epoch,
		reset_source: backoff.reset_source,
		warning: backoff.warning,
	}) {
		let _ = store_error;

		tracing::warn!(
			project_id = context.config.service_id(),
			"Failed to persist recovery tracker backoff; sensitive runtime details were withheld."
		);
	}

	Some(recovery_tracker_backoff_message(
		context.config.service_id(),
		sync_phase,
		backoff.reset_unix_epoch,
		backoff.reset_unix_epoch.saturating_sub(now_unix_epoch),
	))
}

fn recovery_tracker_backoff_from_message(
	message: &str,
	now_unix_epoch: i64,
) -> Option<RecoveryTrackerBackoff<'static>> {
	if message.contains("Linear connector is rate limited") {
		let (reset_unix_epoch, reset_source) =
			match parse_recovery_rate_limit_reset_unix_epoch(message) {
				Some(reset) if reset > now_unix_epoch => (reset, "linear"),
				_ => {
					(now_unix_epoch.saturating_add(LINEAR_RATE_LIMIT_BACKOFF_SECS), "local_default")
				},
			};

		return Some(RecoveryTrackerBackoff {
			quota_class: "linear_graphql_rate_limit",
			reset_unix_epoch,
			reset_source,
			warning: LINEAR_RATE_LIMIT_BACKOFF_WARNING,
		});
	}
	if message.contains("Linear connector timed out") {
		return Some(RecoveryTrackerBackoff {
			quota_class: "linear_graphql_timeout",
			reset_unix_epoch: now_unix_epoch.saturating_add(LINEAR_TRANSIENT_TIMEOUT_BACKOFF_SECS),
			reset_source: "local_transient_timeout",
			warning: LINEAR_TRANSIENT_TIMEOUT_BACKOFF_WARNING,
		});
	}

	None
}

fn parse_recovery_rate_limit_reset_unix_epoch(message: &str) -> Option<i64> {
	let reset = message.split("rate limited until `").nth(1)?.split('`').next()?;

	reset.parse().ok()
}

fn recovery_tracker_backoff_message(
	service_id: &str,
	sync_phase: &str,
	reset_unix_epoch: i64,
	retry_after_seconds: i64,
) -> String {
	format!(
		"Linear connector is in backoff for project `{service_id}`; recovery skipped tracker reads for `{sync_phase}` until unix_epoch={reset_unix_epoch} (retry_after_seconds={retry_after_seconds})."
	)
}
