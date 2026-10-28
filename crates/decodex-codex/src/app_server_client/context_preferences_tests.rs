use crate::app_server_client::context_preferences;

#[test]
fn mcp_startup_projects_only_known_user_servers_and_never_credentials() {
	let user = serde_json::json!({
		"docs.v2":{"url":"http://localhost","bearer_token":"synthetic-secret"},
		"unknown":{"startup_readiness":"future-value"}
	});
	let effective =
		serde_json::json!({"docs.v2":{"startup_readiness":"catalog"},"project-only":{}});

	assert_eq!(
		context_preferences::mcp_startup(&user, &effective),
		vec![("docs.v2".into(), true, None)]
	);
}

#[test]
fn mcp_startup_write_changes_only_a_quoted_server_readiness_leaf() {
	let mut valid = serde_json::json!({"filePath":"/home/config.toml","expectedVersion":"v1","reloadUserConfig":false,"edits":[{"keyPath":"mcp_servers.\"docs.v2\".startup_readiness","value":"catalog","mergeStrategy":"replace"}]});

	assert!(context_preferences::is_context_preference_write(&valid));
	valid["edits"][0]["value"] = serde_json::json!("connection");
	assert!(context_preferences::is_context_preference_write(&valid));

	for path in [
		"mcp_servers.docs.v2.startup_readiness",
		"mcp_servers.\"docs.v2\".required",
		"mcp_servers.\"docs.v2\".startup_readiness.extra",
	] {
		let mut edit = valid.clone();
		edit["edits"][0]["keyPath"] = serde_json::json!(path);
		assert!(!context_preferences::is_context_preference_write(&edit));
	}
	valid["edits"][0]["value"] = serde_json::json!(true);
	assert!(!context_preferences::is_context_preference_write(&valid));
}

#[test]
fn threshold_only_configuration_does_not_enable_async_compaction() {
	assert_eq!(
		context_preferences::enabled(
			&serde_json::json!({"model_async_compact_threshold_percent":70}),
			0
		)
		.unwrap(),
		None
	);
	assert_eq!(
		context_preferences::enabled(
			&serde_json::json!({"enabled":false,"model_async_compact_threshold_percent":70}),
			0
		)
		.unwrap(),
		Some(false)
	);
	assert!(context_preferences::enabled(&serde_json::json!({"enabled":true}), 1).is_err());
}

#[test]
fn context_write_admits_only_a_versioned_boolean_preference() {
	let valid = serde_json::json!({"filePath":"/home/config.toml","expectedVersion":"v1","reloadUserConfig":false,"edits":[{"keyPath":"features.async_compaction","value":false,"mergeStrategy":"replace"}]});

	for key in [
		"features.async_compaction",
		"features.async_compaction.enabled",
		"features.retain_client_tool_outputs",
	] {
		let mut edit = valid.clone();

		edit["edits"][0]["keyPath"] = serde_json::json!(key);

		assert!(context_preferences::is_context_preference_write(&edit));
	}
	for (field, value) in [
		("keyPath", serde_json::json!("features.multi_agent_v2.disable_native_tools")),
		("value", serde_json::json!({"enabled":true})),
		("mergeStrategy", serde_json::json!("upsert")),
	] {
		let mut edit = valid.clone();

		edit["edits"][0][field] = value;

		assert!(!context_preferences::is_context_preference_write(&edit));
	}
	for field in ["expectedVersion", "reloadUserConfig"] {
		let mut edit = valid.clone();

		edit.as_object_mut().unwrap().remove(field);

		assert!(!context_preferences::is_context_preference_write(&edit));
	}

	let mut edit = valid;

	edit["reloadUserConfig"] = serde_json::json!(true);

	assert!(!context_preferences::is_context_preference_write(&edit));
}
