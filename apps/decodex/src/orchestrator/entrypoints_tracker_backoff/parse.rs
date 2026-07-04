use std::time::{Duration, Instant};

use color_eyre::Report;
use time::OffsetDateTime;

use crate::orchestrator::{
	TRACKER_RATE_LIMIT_BACKOFF_SECS, TRACKER_RATE_LIMIT_WARNING,
	TRACKER_TRANSIENT_TIMEOUT_BACKOFF_SECS, TRACKER_TRANSIENT_TIMEOUT_WARNING,
	TrackerConnectorBackoff,
};

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

fn parse_linear_rate_limit_reset_unix_epoch(message: &str) -> Option<i64> {
	let reset = message.split("rate limited until `").nth(1)?.split('`').next()?;

	reset.parse().ok()
}
