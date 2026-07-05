use color_eyre::Report;

use crate::{
	agent::{
		app_server::{
			self,
			tests::{
				AppServerCapabilityPreflightFailure, AppServerCapabilityPreflightReport,
				REQUEST_TIMEOUT, RunRecorder,
			},
		},
		json_rpc::AppServerOutputTimeout,
	},
	state::StateStore,
};

#[test]
fn plugin_list_preflight_uses_local_marketplaces() {
	let params = app_server::plugin_list_params_for_preflight("/tmp/worktree");
	let serialized = serde_json::to_value(&params).expect("plugin params should serialize");

	assert_eq!(serialized["cwds"], serde_json::json!(["/tmp/worktree"]));
	assert_eq!(serialized["marketplaceKinds"], serde_json::json!(["local"]));
}

#[test]
fn plugin_list_preflight_timeout_retries_once_before_success() {
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let mut recorder = RunRecorder::new(&state_store, "run-1", 1, None);
	let report = AppServerCapabilityPreflightReport::new();
	let mut attempts = 0;
	let response = app_server::preflight_request_with_timeout_retry(
		&mut recorder,
		&report,
		"plugin/list",
		REQUEST_TIMEOUT,
		2,
		|| {
			attempts += 1;

			if attempts == 1 { Err(Report::new(AppServerOutputTimeout)) } else { Ok("plugins-ok") }
		},
	)
	.expect("second plugin/list attempt should recover");

	assert_eq!(response, "plugins-ok");
	assert_eq!(attempts, 2);
	assert_eq!(state_store.event_count("run-1").expect("event count should load"), 0);
}

#[test]
fn plugin_list_preflight_timeout_failure_is_typed_retryable_timeout() {
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let mut recorder = RunRecorder::new(&state_store, "run-1", 1, None);
	let report = AppServerCapabilityPreflightReport::new();
	let mut attempts = 0;
	let error = app_server::preflight_request_with_timeout_retry::<(), _>(
		&mut recorder,
		&report,
		"plugin/list",
		REQUEST_TIMEOUT,
		2,
		|| {
			attempts += 1;

			Err(Report::new(AppServerOutputTimeout))
		},
	)
	.expect_err("exhausted plugin/list timeout should fail preflight");
	let failure = error
		.downcast_ref::<AppServerCapabilityPreflightFailure>()
		.expect("plugin/list timeout should be typed");
	let check = &failure.report().checks()[0];
	let timeout_seconds = REQUEST_TIMEOUT.as_secs().to_string();

	assert_eq!(attempts, 2);
	assert_eq!(failure.error_class(), "app_server_plugin_list_timeout");
	assert!(failure.is_retryable_timeout());
	assert!(failure.to_string().contains("app_server_preflight_failed"));
	assert!(failure.to_string().contains("plugin/list"));
	assert!(failure.to_string().contains("timed out"));
	assert!(failure.retry_next_action().contains("retry app-server preflight automatically"));
	assert!(failure.report().has_blockers());
	assert_eq!(check.name, "plugins");
	assert_eq!(check.status, app_server::AppServerCapabilityPreflightStatus::Blocked);
	assert_eq!(check.details.get("failure_reason").map(String::as_str), Some("timeout"));
	assert_eq!(check.details.get("attempt_count").map(String::as_str), Some("2"));
	assert_eq!(check.details.get("retry_count").map(String::as_str), Some("1"));
	assert_eq!(
		check.details.get("timeout_seconds").map(String::as_str),
		Some(timeout_seconds.as_str())
	);
	assert_eq!(state_store.event_count("run-1").expect("event count should load"), 1);
}
