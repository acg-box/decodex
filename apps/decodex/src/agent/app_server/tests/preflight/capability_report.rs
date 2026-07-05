use std::collections::BTreeMap;

use crate::agent::app_server::{
	self,
	protocol::{McpServerStatusSummary, ModelSummary},
	tests::{
		AppServerCapabilityPreflightReport, ModelProviderCapabilitiesReadResponse,
		PluginListResponse, RuntimeConfigSummary, SkillsListResponse,
	},
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
		data: vec![app_server::protocol::SkillsListEntry {
			cwd: String::from("/tmp/worktree"),
			errors: Vec::new(),
			skills: vec![app_server::protocol::SkillMetadata {
				enabled: true,
				name: String::from("codebase:work"),
				scope: String::from("user"),
			}],
		}],
	};
	let plugins = PluginListResponse {
		marketplaces: vec![app_server::protocol::PluginMarketplaceEntry {
			name: String::from("curated"),
			plugins: vec![app_server::protocol::PluginSummary {
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

	app_server::record_config_preflight(&mut report, &config);
	app_server::record_model_preflight(&mut report, &config, &models);
	app_server::record_model_provider_preflight(&mut report, &capabilities);
	app_server::record_skills_preflight(&mut report, "/tmp/worktree", &skills);
	app_server::record_plugin_preflight(&mut report, &plugins);
	app_server::record_mcp_preflight(&mut report, &mcp);

	assert!(!report.has_blockers());
	assert_eq!(report.checks().len(), 6);
	assert!(
		report
			.checks()
			.iter()
			.all(|check| { check.status == app_server::AppServerCapabilityPreflightStatus::Ok })
	);

	let serialized = serde_json::to_value(&report).expect("report should serialize");

	assert_eq!(serialized["checks"][0]["status"], "ok");
	assert_eq!(serialized["checks"][1]["details"]["configured_model"], "gpt-5.4");
}

#[test]
fn capability_preflight_report_allows_enabled_skills_with_scan_diagnostics() {
	let skills = SkillsListResponse {
		data: vec![app_server::protocol::SkillsListEntry {
			cwd: String::from("/tmp/worktree"),
			errors: vec![app_server::protocol::SkillErrorInfo {
				message: String::from("name: exceeds maximum length of 64 characters"),
				path: String::from(
					"/tmp/plugins/build-web-data-visualization/skills/chart/SKILL.md",
				),
			}],
			skills: vec![app_server::protocol::SkillMetadata {
				enabled: true,
				name: String::from("codebase:work"),
				scope: String::from("user"),
			}],
		}],
	};
	let mut report = AppServerCapabilityPreflightReport::new();

	app_server::record_skills_preflight(&mut report, "/tmp/worktree", &skills);

	assert!(!report.has_blockers());
	assert_eq!(report.checks()[0].status, app_server::AppServerCapabilityPreflightStatus::Ok);
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
		data: vec![app_server::protocol::SkillsListEntry {
			cwd: String::from("/tmp/worktree"),
			errors: vec![app_server::protocol::SkillErrorInfo {
				message: String::from("bad skill metadata"),
				path: String::from("/tmp/worktree/.codex/skills/bad/SKILL.md"),
			}],
			skills: Vec::new(),
		}],
	};
	let plugins = PluginListResponse {
		marketplaces: Vec::new(),
		marketplace_load_errors: vec![app_server::protocol::MarketplaceLoadErrorInfo {
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

	app_server::record_model_preflight(&mut report, &config, &models);
	app_server::record_skills_preflight(&mut report, "/tmp/worktree", &skills);
	app_server::record_plugin_preflight(&mut report, &plugins);
	app_server::record_mcp_preflight(&mut report, &mcp);

	assert!(report.has_blockers());
	assert_eq!(
		report.blocker_summary(),
		"model: configured model was not present in model/list.; skills: skills/list returned no enabled skills. first_error_path=/tmp/worktree/.codex/skills/bad/SKILL.md; first_error=bad skill metadata; plugins: plugin/list returned marketplace load errors. first_error_path=/tmp/plugins.json; first_error=invalid marketplace; mcp: mcpServerStatus/list returned MCP servers that are not logged in."
	);
}
