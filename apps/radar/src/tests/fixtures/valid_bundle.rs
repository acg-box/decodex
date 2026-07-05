use serde_json::Value;

pub(crate) fn valid_bundle() -> Value {
	serde_json::json!({
		"schema": "github_change_bundle/v1",
		"repo": "openai/codex",
		"analysis_mode": "pr_first",
		"default_branch": "main",
		"primary_pr": {
			"number": 22_414,
			"title": "Add Unix socket endpoint support",
			"body": "",
			"state": "merged",
			"merged_at": "2026-06-01T00:00:00Z",
			"labels": [],
			"url": "https://github.com/openai/codex/pull/22414"
		},
		"commits": [
			{
				"sha": "abc123",
				"message": "Add Unix socket endpoint support",
				"url": "https://github.com/openai/codex/commit/abc123"
			}
		],
		"files": [
			{
				"path": "codex-rs/app-server/src/lib.rs",
				"status": "modified",
				"additions": 12,
				"deletions": 1
			}
		]
	})
}
