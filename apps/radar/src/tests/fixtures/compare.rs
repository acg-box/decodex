use serde_json::Value;

pub(crate) fn compare() -> Value {
	serde_json::json!({
		"status": "ahead",
		"ahead_by": 1,
		"total_commits": 1,
		"url": "https://github.com/openai/codex/compare/rust-v0.1.0...rust-v0.2.0-alpha.1",
		"commit_shas": ["abc123"],
		"pr_numbers": [22_414]
	})
}
