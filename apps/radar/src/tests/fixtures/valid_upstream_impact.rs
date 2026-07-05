use serde_json::Value;

pub(crate) fn valid_upstream_impact() -> Value {
	serde_json::json!({
		"schema": "upstream_impact/v1",
		"slug": "openai-codex-pr-22414",
		"repo": "openai/codex",
		"source_refs": {
			"items": [
				{
					"kind": "pull_request",
					"title": "Add Unix socket endpoint support",
					"url": "https://github.com/openai/codex/pull/22414"
				}
			]
		},
		"observed_change": "Remote Codex can use Unix socket endpoints.",
		"public_signal_decision": "publish",
		"control_plane_impact": "candidate",
		"publisher_angle": "operator_impact",
		"confidence": "confirmed",
		"evidence": ["PR #22414 updates app-server endpoint handling."]
	})
}
