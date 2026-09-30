use serde_json::Value;

pub(crate) fn release(tag_name: &str, prerelease: bool) -> Value {
	serde_json::json!({
		"tag_name": tag_name,
		"name": tag_name,
		"published_at": "2026-06-01T00:00:00Z",
		"url": format!("https://github.com/openai/codex/releases/tag/{}", crate::percent_encode(tag_name)),
		"prerelease": prerelease
	})
}
