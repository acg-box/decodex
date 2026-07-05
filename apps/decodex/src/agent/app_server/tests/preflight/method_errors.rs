use std::collections::BTreeMap;

use crate::{
	agent::app_server::{
		self,
		tests::{
			AppServerCapabilityPreflightFailure, AppServerCapabilityPreflightReport, RunRecorder,
		},
	},
	prelude::eyre,
	state::StateStore,
};

#[test]
fn capability_preflight_method_error_is_typed_operator_blocker() {
	let mut report = AppServerCapabilityPreflightReport::new();

	report.push_ok(
		"config",
		"config/read returned effective runtime configuration.",
		BTreeMap::new(),
	);

	let failure = AppServerCapabilityPreflightFailure::method_failed(
		"model/list",
		String::from("`model/list` failed with -32601: Method not found"),
		report,
	);

	assert_eq!(failure.error_class(), "app_server_introspection_method_failed");
	assert!(!failure.is_retryable_timeout());
	assert!(failure.to_string().contains("model/list"));
	assert!(failure.to_string().contains("Method not found"));
	assert_eq!(failure.report().checks().len(), 1);
}

#[test]
fn capability_preflight_request_error_records_method_blocker() {
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let mut recorder = RunRecorder::new(&state_store, "run-1", 1, None);
	let report = AppServerCapabilityPreflightReport::new();
	let error =
		app_server::preflight_request::<(), _>(&mut recorder, &report, "model/list", || {
			Err(eyre::eyre!("JSON-RPC error -32601: Method not found"))
		})
		.expect_err("unsupported app-server method should fail preflight");
	let failure = error
		.downcast_ref::<AppServerCapabilityPreflightFailure>()
		.expect("preflight request error should be typed");

	assert_eq!(failure.error_class(), "app_server_introspection_method_failed");
	assert!(failure.to_string().contains("model/list"));
	assert!(failure.to_string().contains("Method not found"));
	assert!(failure.report().has_blockers());
	assert_eq!(state_store.event_count("run-1").expect("event count should load"), 1);
}
