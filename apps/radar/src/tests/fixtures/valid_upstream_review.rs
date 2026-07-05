use serde_json::Value;

pub(crate) fn valid_upstream_review() -> Value {
	serde_json::json!({
		"schema": "upstream_review/v1",
		"slug": "openai-codex-pr-22414",
		"repo": "openai/codex",
		"subject": {
			"subject_kind": "pr",
			"subject_id": "22414",
			"commit_shas": ["abc123"]
		},
		"source_refs": {
			"items": [
				{
					"kind": "pull_request",
					"title": "Add Unix socket endpoint support",
					"url": "https://github.com/openai/codex/pull/22414"
				}
			]
		},
		"reviewed_at": "2026-06-01T00:00:00Z",
		"observed_change": "Remote Codex can use Unix socket endpoints.",
		"changed_surfaces": ["app server"],
		"confidence": "confirmed",
		"evidence": ["PR #22414 updates app-server endpoint handling."],
		"next_actions": [
			{
				"type": "upstream_impact",
				"reason": "Transport behavior can affect Decodex."
			}
		]
	})
}
