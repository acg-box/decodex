//! Transient provider wait state. Native turn lifecycle owns its duration.
use super::{AppServerClient, ServerEvent};
use std::{
	collections::HashMap,
	sync::{Arc, Mutex},
};

#[derive(Clone, Default)]
pub(super) struct ProviderWait(Arc<Mutex<HashMap<String, (String, bool)>>>);
impl ProviderWait {
	pub(super) fn clear(&self) {
		if let Ok(mut turns) = self.0.lock() {
			turns.clear();
		}
	}

	pub(super) fn buffering_turn(&self, thread: &str) -> Option<String> {
		let turns = self.0.lock().ok()?;
		let (turn, buffering) = turns.get(thread)?;

		buffering.then(|| turn.clone())
	}

	pub(super) fn observe(&self, event: &ServerEvent) {
		let ServerEvent::Notification { method, params } = event else { return };
		let Some(thread) = params["threadId"].as_str().filter(|id| !id.is_empty()) else { return };
		let Ok(mut turns) = self.0.lock() else { return };

		match method.as_str() {
			"turn/started" => {
				if let Some(turn) = params["turn"]["id"].as_str().filter(|id| !id.is_empty()) {
					turns.insert(thread.into(), (turn.into(), false));
				}
			},
			"model/safetyBuffering/updated" => {
				if let Some((turn, active)) = turns.get_mut(thread)
					&& params["turnId"].as_str() == Some(turn.as_str())
					&& let Some(show) = params["showBufferingUi"].as_bool()
				{
					*active = show;
				}
			},
			"turn/completed" => {
				if turns
					.get(thread)
					.is_some_and(|(turn, _)| params["turn"]["id"].as_str() == Some(turn.as_str()))
				{
					turns.remove(thread);
				}
			},
			"thread/closed" | "thread/archived" | "thread/deleted" | "thread/reverted" => {
				turns.remove(thread);
			},
			_ => {},
		}
	}
}

impl AppServerClient {
	/// Exact live turn with an explicit native safety buffering notice. No history replay.
	pub fn safety_buffering_turn(&self, thread: &str) -> Option<String> {
		if *self.closed.borrow() || self.outbound.is_closed() {
			return None;
		}

		self.server_requests.safety_buffering_turn(thread)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use serde_json::json;
	use tokio::io::AsyncWriteExt;

	#[tokio::test]
	async fn safety_buffering_is_exact_live_connection_state_not_replayed_history() {
		let (local, remote) = tokio::io::duplex(8_192);
		let (r, w) = tokio::io::split(local);
		let (client, mut events) = AppServerClient::from_io(r, w);
		let (_r, mut w) = tokio::io::split(remote);

		for (method, params, expected) in [
			(
				"model/safetyBuffering/updated",
				json!({"threadId":"t","turnId":"old","showBufferingUi":true}),
				None,
			),
			("turn/started", json!({"threadId":"t","turn":{"id":"one"}}), None),
			(
				"model/verification",
				json!({"threadId":"t","turnId":"one","verifications":["trustedAccessForCyber"]}),
				None,
			),
			(
				"model/safetyBuffering/updated",
				json!({"threadId":"other","turnId":"one","showBufferingUi":true}),
				None,
			),
			(
				"model/safetyBuffering/updated",
				json!({"threadId":"t","turnId":"old","showBufferingUi":true}),
				None,
			),
			(
				"model/safetyBuffering/updated",
				json!({"threadId":"t","turnId":"one","showBufferingUi":true}),
				Some("one"),
			),
			(
				"model/safetyBuffering/updated",
				json!({"threadId":"t","turnId":"one","showBufferingUi":false}),
				None,
			),
			(
				"model/safetyBuffering/updated",
				json!({"threadId":"t","turnId":"one","showBufferingUi":true}),
				Some("one"),
			),
			(
				"turn/completed",
				json!({"threadId":"t","turn":{"id":"old","status":"completed"}}),
				Some("one"),
			),
			(
				"turn/completed",
				json!({"threadId":"t","turn":{"id":"one","status":"completed"}}),
				None,
			),
			(
				"model/safetyBuffering/updated",
				json!({"threadId":"t","turnId":"one","showBufferingUi":true}),
				None,
			),
			("turn/started", json!({"threadId":"t","turn":{"id":"two"}}), None),
			(
				"model/safetyBuffering/updated",
				json!({"threadId":"t","turnId":"two","showBufferingUi":true}),
				Some("two"),
			),
			("thread/reverted", json!({"threadId":"t"}), None),
			("turn/started", json!({"threadId":"t","turn":{"id":"three"}}), None),
			(
				"model/safetyBuffering/updated",
				json!({"threadId":"t","turnId":"three","showBufferingUi":true}),
				Some("three"),
			),
		] {
			w.write_all(format!("{}\n", json!({"method":method,"params":params})).as_bytes())
				.await
				.unwrap();

			assert!(matches!(events.recv().await, Some(ServerEvent::Notification { .. })));
			assert_eq!(client.safety_buffering_turn("t").as_deref(), expected, "{method}");
		}

		drop(w);
		drop(_r);

		assert!(matches!(events.recv().await, Some(ServerEvent::Closed(_))));
		assert_eq!(client.safety_buffering_turn("t"), None);
	}
}
