use std::collections::BTreeMap;

use color_eyre::Report;

use crate::{
	agent::{
		app_server::{
			protocol::{McpServerStatusSummary, ModelSummary},
			tests::{
				AppServerCapabilityPreflightFailure, AppServerCapabilityPreflightReport,
				ModelProviderCapabilitiesReadResponse, PluginListResponse, REQUEST_TIMEOUT,
				RunRecorder, RuntimeConfigSummary, SkillsListResponse,
			},
		},
		json_rpc::AppServerOutputTimeout,
	},
	prelude::eyre,
	state::StateStore,
};
#[test]
fn capability_preflight_report_accepts_available_runtime_state() {
	let config = RuntimeConfigSummary {
		model: Some(String::from("gpt-5.4")),
		model_provider: Some(String::from("openai")),
		approval_policy: Some(serde_json::json!("never")),
		sandbox_mode: Some(serde_json::json!("workspaceWrite")),
	};
	let models = vec![ModelSummary {
		id: String::from("model-gpt-5.4"),
		model: String::from("gpt-5.4"),
		display_name: String::from("GPT-5.4"),
		is_default: true,
		hidden: false,
	}];
	let capabilities = ModelProviderCapabilitiesReadResponse {
		image_generation: true,
		namespace_tools: true,
		web_search: true,
	};
	let skills = SkillsListResponse {
		data: vec![super::super::protocol::SkillsListEntry {
			cwd: String::from("/tmp/worktree"),
			errors: Vec::new(),
			skills: vec![super::super::protocol::SkillMetadata {
				enabled: true,
				name: String::from("codebase:work"),
				scope: String::from("user"),
			}],
		}],
	};
	let plugins = PluginListResponse {
		marketplaces: vec![super::super::protocol::PluginMarketplaceEntry {
			name: String::from("curated"),
			plugins: vec![super::super::protocol::PluginSummary {
				enabled: true,
				id: String::from("github"),
				installed: true,
				name: String::from("GitHub"),
			}],
		}],
		marketplace_load_errors: Vec::new(),
	};
	let mcp = vec![McpServerStatusSummary {
		auth_status: String::from("bearerToken"),
		name: String::from("linear"),
		tools: BTreeMap::from([(String::from("issue_transition"), serde_json::json!({}))]),
	}];
	let mut report = AppServerCapabilityPreflightReport::new();

	super::record_config_preflight(&mut report, &config);
	super::record_model_preflight(&mut report, &config, &models);
	super::record_model_provider_preflight(&mut report, &capabilities);
	super::record_skills_preflight(&mut report, "/tmp/worktree", &skills);
	super::record_plugin_preflight(&mut report, &plugins);
	super::record_mcp_preflight(&mut report, &mcp);

	assert!(!report.has_blockers());
	assert_eq!(report.checks().len(), 6);
	assert!(
		report
			.checks()
			.iter()
			.all(|check| { check.status == super::super::AppServerCapabilityPreflightStatus::Ok })
	);

	let serialized = serde_json::to_value(&report).expect("report should serialize");

	assert_eq!(serialized["checks"][0]["status"], "ok");
	assert_eq!(serialized["checks"][1]["details"]["configured_model"], "gpt-5.4");
}

#[test]
fn capability_preflight_report_allows_enabled_skills_with_scan_diagnostics() {
	let skills = SkillsListResponse {
		data: vec![super::super::protocol::SkillsListEntry {
			cwd: String::from("/tmp/worktree"),
			errors: vec![super::super::protocol::SkillErrorInfo {
				message: String::from("name: exceeds maximum length of 64 characters"),
				path: String::from(
					"/tmp/plugins/build-web-data-visualization/skills/chart/SKILL.md",
				),
			}],
			skills: vec![super::super::protocol::SkillMetadata {
				enabled: true,
				name: String::from("codebase:work"),
				scope: String::from("user"),
			}],
		}],
	};
	let mut report = AppServerCapabilityPreflightReport::new();

	super::record_skills_preflight(&mut report, "/tmp/worktree", &skills);

	assert!(!report.has_blockers());
	assert_eq!(report.checks()[0].status, super::super::AppServerCapabilityPreflightStatus::Ok);
	assert_eq!(
		report.checks()[0].summary,
		"skills/list returned enabled skills with scan diagnostics."
	);
	assert_eq!(report.checks()[0].details["enabled_skill_count"], "1");
	assert_eq!(report.checks()[0].details["error_count"], "1");
	assert_eq!(
		report.checks()[0].details["first_error"],
		"name: exceeds maximum length of 64 characters"
	);
}

#[test]
fn capability_preflight_report_blocks_missing_runtime_state() {
	let config = RuntimeConfigSummary {
		model: Some(String::from("missing-model")),
		model_provider: Some(String::from("openai")),
		approval_policy: None,
		sandbox_mode: None,
	};
	let models = vec![ModelSummary {
		id: String::from("model-gpt-5.4"),
		model: String::from("gpt-5.4"),
		display_name: String::from("GPT-5.4"),
		is_default: true,
		hidden: false,
	}];
	let skills = SkillsListResponse {
		data: vec![super::super::protocol::SkillsListEntry {
			cwd: String::from("/tmp/worktree"),
			errors: vec![super::super::protocol::SkillErrorInfo {
				message: String::from("bad skill metadata"),
				path: String::from("/tmp/worktree/.codex/skills/bad/SKILL.md"),
			}],
			skills: Vec::new(),
		}],
	};
	let plugins = PluginListResponse {
		marketplaces: Vec::new(),
		marketplace_load_errors: vec![super::super::protocol::MarketplaceLoadErrorInfo {
			marketplace_path: String::from("/tmp/plugins.json"),
			message: String::from("invalid marketplace"),
		}],
	};
	let mcp = vec![McpServerStatusSummary {
		auth_status: String::from("notLoggedIn"),
		name: String::from("linear"),
		tools: BTreeMap::new(),
	}];
	let mut report = AppServerCapabilityPreflightReport::new();

	super::record_model_preflight(&mut report, &config, &models);
	super::record_skills_preflight(&mut report, "/tmp/worktree", &skills);
	super::record_plugin_preflight(&mut report, &plugins);
	super::record_mcp_preflight(&mut report, &mcp);

	assert!(report.has_blockers());
	assert_eq!(
		report.blocker_summary(),
		"model: configured model was not present in model/list.; skills: skills/list returned no enabled skills. first_error_path=/tmp/worktree/.codex/skills/bad/SKILL.md; first_error=bad skill metadata; plugins: plugin/list returned marketplace load errors. first_error_path=/tmp/plugins.json; first_error=invalid marketplace; mcp: mcpServerStatus/list returned MCP servers that are not logged in."
	);
}

#[test]
fn plugin_list_preflight_uses_local_marketplaces() {
	let params = super::plugin_list_params_for_preflight("/tmp/worktree");
	let serialized = serde_json::to_value(&params).expect("plugin params should serialize");

	assert_eq!(serialized["cwds"], serde_json::json!(["/tmp/worktree"]));
	assert_eq!(serialized["marketplaceKinds"], serde_json::json!(["local"]));
}

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
	let error = super::preflight_request::<(), _>(&mut recorder, &report, "model/list", || {
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

#[test]
fn plugin_list_preflight_timeout_retries_once_before_success() {
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let mut recorder = RunRecorder::new(&state_store, "run-1", 1, None);
	let report = AppServerCapabilityPreflightReport::new();
	let mut attempts = 0;
	let response = super::preflight_request_with_timeout_retry(
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
	let error = super::preflight_request_with_timeout_retry::<(), _>(
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
	assert_eq!(check.status, super::super::AppServerCapabilityPreflightStatus::Blocked);
	assert_eq!(check.details.get("failure_reason").map(String::as_str), Some("timeout"));
	assert_eq!(check.details.get("attempt_count").map(String::as_str), Some("2"));
	assert_eq!(check.details.get("retry_count").map(String::as_str), Some("1"));
	assert_eq!(
		check.details.get("timeout_seconds").map(String::as_str),
		Some(timeout_seconds.as_str())
	);
	assert_eq!(state_store.event_count("run-1").expect("event count should load"), 1);
}

#[test]
fn mcp_preflight_timeout_degrades_to_recorded_ok_check() {
	let error = Report::new(AppServerOutputTimeout);
	let mut report = AppServerCapabilityPreflightReport::new();

	assert!(super::mcp_preflight_can_degrade(&error));

	super::record_mcp_preflight_degraded(&mut report, &error);

	assert!(!report.has_blockers());
	assert_eq!(report.checks().len(), 1);
	assert_eq!(report.checks()[0].name, "mcp");
	assert_eq!(report.checks()[0].status, super::super::AppServerCapabilityPreflightStatus::Ok);
	assert_eq!(
		report.checks()[0].details.get("degraded_reason").map(String::as_str),
		Some("timeout")
	);
	assert!(report.checks()[0].summary.contains("continuing"));
}
