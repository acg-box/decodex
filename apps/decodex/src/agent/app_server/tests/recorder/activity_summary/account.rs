use crate::{
	agent::app_server::tests::recorder::{RunRecorder, TempDir},
	state::{self, StateStore},
};

#[test]
fn recorder_summarizes_v2_account_rate_limit_notifications() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let marker_path = temp_dir.path().to_path_buf();
	let mut recorder = RunRecorder::new(&state_store, "run-1", 1, Some(&marker_path));

	recorder
		.record(
			"account/rateLimits/updated",
			r#"{"method":"account/rateLimits/updated","params":{"rateLimits":{"planType":"pro","rateLimitReachedType":"workspace_member_usage_limit_reached","primary":{"usedPercent":100}}}}"#,
		)
		.expect("rate limit protocol event should record");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");
	let summary = marker.protocol_activity().expect("protocol activity should be captured");
	let event = summary.recent_events.first().expect("recent rate limit event should render");

	assert_eq!(summary.rate_limit_status.as_deref(), Some("workspace_member_usage_limit_reached"));
	assert_eq!(event.category, "rate_limit");
	assert_eq!(event.detail.as_deref(), Some("pro/workspace_member_usage_limit_reached"));
}

#[test]
fn recorder_does_not_treat_rate_limit_update_method_as_limit_status() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let marker_path = temp_dir.path().to_path_buf();
	let mut recorder = RunRecorder::new(&state_store, "run-1", 1, Some(&marker_path));

	recorder
		.record(
			"account/rateLimits/updated",
			r#"{"method":"account/rateLimits/updated","params":{"rateLimits":{"planType":"pro","rateLimitReachedType":null,"primary":{"usedPercent":12}}}}"#,
		)
		.expect("rate limit update event should record");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");
	let summary = marker.protocol_activity().expect("protocol activity should be captured");

	assert_eq!(summary.rate_limit_status, None);
	assert_eq!(
		summary.recent_events.first().and_then(|event| event.detail.as_deref()),
		Some("pro")
	);
}

#[test]
fn recorder_summarizes_wrapped_account_protocol_activity() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let marker_path = temp_dir.path().to_path_buf();
	let mut recorder = RunRecorder::new(&state_store, "run-1", 1, Some(&marker_path));

	recorder
		.record(
			"account/update",
			r#"{"method":"account/update","params":{"planType":"pro","refreshStatus":"refreshed"}}"#,
		)
		.expect("account protocol event should record");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");
	let summary = marker.protocol_activity().expect("protocol activity should be captured");
	let event = summary.recent_events.first().expect("recent account event should render");

	assert_eq!(event.category, "account");
	assert_eq!(event.detail.as_deref(), Some("pro/refreshed"));
}
