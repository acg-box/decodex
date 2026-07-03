use crate::orchestrator::tests::operator::status::{
	Duration, Instant, OffsetDateTime, eyre, orchestrator,
};

#[test]
fn tracker_rate_limit_error_enters_control_plane_backoff() {
	let now = Instant::now();
	let error = eyre::eyre!(
		"Linear connector is rate limited: Rate limit exceeded. Only 2500 requests are allowed per 1 hour."
	);
	let backoff_until = orchestrator::tracker_connector_backoff(&error, now, "control_plane_tick")
		.expect("rate limit should create backoff");

	assert!(backoff_until.until > now);
}

#[test]
fn tracker_timeout_error_enters_transient_control_plane_backoff() {
	let now = Instant::now();
	let error = eyre::eyre!("Linear connector timed out during GraphQL request: deadline elapsed");
	let backoff = orchestrator::tracker_connector_backoff(&error, now, "control_plane_tick")
		.expect("timeout should create transient tracker backoff");

	assert!(backoff.until >= now + Duration::from_secs(59));
	assert!(backoff.until <= now + Duration::from_secs(61));
	assert_eq!(backoff.quota_class, "linear_graphql_timeout");
	assert_eq!(backoff.reset_source, "local_transient_timeout");
	assert_eq!(backoff.sync_phase, "control_plane_tick");
	assert_eq!(backoff.warning, orchestrator::TRACKER_TRANSIENT_TIMEOUT_WARNING);
}

#[test]
fn tracker_rate_limit_error_uses_reset_timestamp_when_available() {
	let now = Instant::now();
	let reset_unix_epoch = OffsetDateTime::now_utc().unix_timestamp() + 30;
	let error = eyre::eyre!(
		"Linear connector is rate limited until `{reset_unix_epoch}`: API rate limit exceeded"
	);
	let backoff_until = orchestrator::tracker_connector_backoff(&error, now, "control_plane_tick")
		.expect("rate limit reset should create backoff");

	assert!(backoff_until.until >= now + Duration::from_secs(29));
	assert!(backoff_until.until <= now + Duration::from_secs(31));
	assert_eq!(backoff_until.quota_class, "linear_graphql_rate_limit");
	assert_eq!(backoff_until.reset_unix_epoch, reset_unix_epoch);
	assert_eq!(backoff_until.reset_source, "linear");
	assert_eq!(backoff_until.sync_phase, "control_plane_tick");
	assert_eq!(backoff_until.warning, orchestrator::TRACKER_RATE_LIMIT_WARNING);
}
