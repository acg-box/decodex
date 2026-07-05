use crate::agent::app_server::{self, tests};

#[test]
fn matches_thread_id_from_supported_notification_shapes() {
	for message in [
		tests::notification_message(
			"thread/started",
			serde_json::json!({
				"thread": {
					"id": "thread-1",
				}
			}),
		),
		tests::notification_message(
			"turn/completed",
			serde_json::json!({
				"threadId": "thread-1",
				"turn": {
					"id": "turn-1",
					"status": "completed",
					"error": null,
				}
			}),
		),
	] {
		assert!(app_server::targets_thread(&message, Some("thread-1")));
		assert!(!app_server::targets_thread(&message, Some("thread-2")));
	}
}
