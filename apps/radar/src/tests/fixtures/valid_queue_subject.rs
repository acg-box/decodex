use serde_json::Value;

pub(crate) fn valid_queue_subject() -> Value {
	serde_json::json!({
		"subject_kind": "pr",
		"subject_id": "22414",
		"title": "Add Unix socket endpoint support",
		"url": "https://github.com/openai/codex/pull/22414",
		"source_state": "merged",
		"commit_shas": ["abc123"],
		"changed_file_count": 1,
		"sample_paths": ["codex-rs/app-server/src/lib.rs"],
		"surface_hints": ["app_server_protocol"],
		"attention_flags": [],
		"review_priority": "high",
		"review_reason": "Transport behavior changed.",
		"next_step": "ai_review_required"
	})
}
