use serde_json::Value;

pub(crate) fn valid_config_feature_catalog() -> Value {
	serde_json::json!({
		"schema": "codex_config_feature_catalog/v1",
		"source_url": "https://raw.githubusercontent.com/openai/codex/main/codex-rs/core/config.schema.json",
		"generated_at": "2026-06-02T00:00:00Z",
		"feature_count": 1,
		"features": [
			{
				"name": "multi_agent_v2",
				"config_path": "features.multi_agent_v2",
				"toml_assignment": "multi_agent_v2 = true",
				"toml_snippet": "[features]\nmulti_agent_v2 = true",
				"cli_enable_flag": "--enable multi_agent_v2",
				"schema_url": "https://raw.githubusercontent.com/openai/codex/main/codex-rs/core/config.schema.json",
				"reference_url": "https://developers.openai.com/codex/config-reference",
				"reference_description": "Enable MultiAgentV2 tools including followup_task; legacy assign_task appears only in older rollout traces.",
				"github_search_url": "https://github.com/openai/codex/search?q=%22multi_agent_v2%22&type=code"
			}
		]
	})
}
