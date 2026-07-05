use serde_json::Value;

pub(crate) fn release(tag_name: &str, prerelease: bool) -> Value {
	serde_json::json!({
		"tag_name": tag_name,
		"name": tag_name,
		"published_at": "2026-06-01T00:00:00Z",
		"url": "https://github.com/openai/codex/releases/tag/rust-v0.1.0",
		"prerelease": prerelease
	})
}
