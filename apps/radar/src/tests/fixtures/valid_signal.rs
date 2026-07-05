use serde_json::Value;

pub(crate) fn valid_signal() -> Value {
	serde_json::json!({
		"schema": "signal_entry/v1",
		"slug": "openai-codex-pr-22414",
		"lane": "github",
		"kind": "capability",
		"title": "Unix sockets for remote Codex",
		"published_at": "2026-06-01T00:00:00Z",
		"summary": "Remote Codex can use Unix socket endpoints.",
		"why_it_matters": "Operators can use local socket transports.",
		"confidence": "confirmed",
		"impact": "medium",
		"proof_points": ["PR #22414 adds endpoint handling."],
		"source_refs": {
			"repo": "openai/codex",
			"pr_url": "https://github.com/openai/codex/pull/22414",
			"items": [
				{
					"kind": "pull_request",
					"title": "Add Unix socket endpoint support",
					"url": "https://github.com/openai/codex/pull/22414"
				}
			]
		}
	})
}
