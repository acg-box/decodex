use serde_json::Value;

use crate::tests::fixtures;

pub(crate) fn valid_review_queue() -> Value {
	serde_json::json!({
		"schema": "upstream_review_queue/v1",
		"repo": "openai/codex",
		"generated_at": "2026-06-01T00:00:00Z",
		"source": {
			"default_branch": "main",
			"search_limit": 40
		},
		"subjects": [fixtures::valid_queue_subject()],
		"counts": {
			"subjects_queued": 1,
			"recent_commits_scanned": 1,
			"published_subjects_seen": 0,
			"critical": 0,
			"high": 1,
			"normal": 0,
			"low": 0
		}
	})
}
