use serde_json::Value;

use crate::tests::fixtures::{self, compare, release};

pub(crate) fn valid_release_delta() -> Value {
	serde_json::json!({
		"schema": "release_delta/v1",
		"repo": "openai/codex",
		"tag_prefix": "rust-v",
		"generated_at": "2026-06-01T00:00:00Z",
		"stable_release": release("rust-v0.1.0", false),
		"prerelease": release("rust-v0.2.0-alpha.1", true),
		"compare": compare(),
		"tracked_signal_slugs": ["openai-codex-pr-22414"],
		"release_options": {
			"stable": [fixtures::release("rust-v0.1.0", false)],
			"preview": [fixtures::release("rust-v0.2.0-alpha.1", true)]
		},
		"comparisons": [
			{
				"stable_tag_name": "rust-v0.1.0",
				"prerelease_tag_name": "rust-v0.2.0-alpha.1",
				"compare": compare(),
				"tracked_signal_slugs": ["openai-codex-pr-22414"]
			}
		]
	})
}
