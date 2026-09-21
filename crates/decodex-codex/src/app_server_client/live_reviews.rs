//! Connection-local continuation evidence. Persisted errors cannot populate this cache.
use super::{ClientError, ServerEvent};
use serde_json::Value;
use std::{
	collections::HashMap,
	sync::{Arc, Mutex},
};

#[derive(Clone, Default)]
pub(super) struct LiveReviews(Arc<Mutex<State>>);

#[derive(Default)]
struct State {
	next: u64,
	entries: HashMap<String, Entry>,
}

#[cfg(test)]
mod tests {
	use super::{super::AppServerClient, *};
	use serde_json::json;
	use tokio::sync::mpsc;

	fn error(thread: &str) -> Value {
		json!({"method":"error","params":{"threadId":thread,"turnId":"failed","willRetry":false,"error":{"codexErrorInfo":"misalignmentPolicyViolation","misalignment":{"detailedExplanation":"Review scope","steer":{"message":"Continue within scope"}}}}})
	}

	#[tokio::test]
	async fn live_review_guard_rejects_queued_changes_before_any_write() {
		for change in [
			json!({"method":"turn/started","params":{"threadId":"thread","turn":{"id":"new"}}}),
			json!({"method":"thread/reverted","params":{"threadId":"thread"}}),
			json!({"method":"thread/closed","params":{"threadId":"thread"}}),
			json!({"method":"item/completed","params":{"threadId":"thread","turnId":"failed","item":{"id":"input","type":"userMessage"}}}),
		] {
			let (incoming, frames) = mpsc::channel(8);
			let (outgoing, mut writes) = mpsc::channel(8);
			let (client, mut events) = AppServerClient::from_framed(1, frames, outgoing).unwrap();
			incoming.send(Ok(error("thread"))).await.unwrap();
			events.recv().await.unwrap();
			let (_, guard) = client.live_misalignment_review("thread", "failed").unwrap();
			incoming.send(Ok(change)).await.unwrap();
			assert!(matches!(
				client.request_with_history("turn/start", json!({}), guard).await,
				Err(ClientError::StaleHistory)
			));
			assert!(writes.try_recv().is_err());
			assert!(client.live_misalignment_review("thread", "failed").is_none());
		}
	}

	#[tokio::test]
	async fn live_review_survives_detail_free_terminal_but_not_disconnect_or_aba() {
		let (incoming, frames) = mpsc::channel(8);
		let (outgoing, mut writes) = mpsc::channel(8);
		let (client, mut events) = AppServerClient::from_framed(1, frames, outgoing).unwrap();
		incoming.send(Ok(error("thread"))).await.unwrap();
		events.recv().await.unwrap();
		let (_, first) = client.live_misalignment_review("thread", "failed").unwrap();
		for frame in [
			json!({"method":"turn/completed","params":{"threadId":"thread","turn":{"id":"failed","status":"failed","error":{"codexErrorInfo":"misalignmentPolicyViolation"}}}}),
			json!({"method":"turn/started","params":{"threadId":"other","turn":{"id":"new"}}}),
		] {
			incoming.send(Ok(frame)).await.unwrap();
			events.recv().await.unwrap();
			assert!(first.is_live());
		}
		incoming
			.send(Ok(json!({"method":"thread/reverted","params":{"threadId":"thread"}})))
			.await
			.unwrap();
		events.recv().await.unwrap();
		incoming.send(Ok(error("thread"))).await.unwrap();
		events.recv().await.unwrap();
		assert!(!first.is_live());
		let (_, current) = client.live_misalignment_review("thread", "failed").unwrap();
		let (other_sender, other_frames) = mpsc::channel(8);
		let (other_writes, mut other_written) = mpsc::channel(8);
		let (other, _events) = AppServerClient::from_framed(1, other_frames, other_writes).unwrap();
		assert!(matches!(
			other.request_with_history("turn/start", json!({}), current.clone()).await,
			Err(ClientError::StaleHistory)
		));
		assert!(other_written.try_recv().is_err());
		drop(other_sender);
		let sender = client.clone();
		let request = tokio::spawn(async move {
			sender.request_with_history("turn/start", json!({}), current.clone()).await
		});
		let wire = writes.recv().await.unwrap();
		incoming
			.send(Ok(json!({"id":wire["id"],"result":{"turn":{"id":"accepted"}}})))
			.await
			.unwrap();
		assert_eq!(request.await.unwrap().unwrap()["turn"]["id"], "accepted");
		let (_, guard) = client.live_misalignment_review("thread", "failed").unwrap();
		drop(incoming);
		events.recv().await.unwrap();
		assert!(!guard.is_live());
		assert!(client.live_misalignment_review("thread", "failed").is_none());
	}
}
struct Entry {
	turn: String,
	serial: u64,
	error: Value,
}

#[derive(Clone)]
pub(super) struct LiveReviewGuard {
	reviews: LiveReviews,
	thread: String,
	serial: u64,
}
impl LiveReviewGuard {
	pub(super) fn is_live(&self) -> bool {
		self.reviews.0.lock().is_ok_and(|state| {
			state.entries.get(&self.thread).is_some_and(|entry| entry.serial == self.serial)
		})
	}
}

impl LiveReviews {
	pub(super) fn clear(&self) {
		if let Ok(mut state) = self.0.lock() {
			state.entries.clear();
		}
	}

	pub(super) fn capture(&self, thread: &str, turn: &str) -> Option<(Value, LiveReviewGuard)> {
		let state = self.0.lock().ok()?;
		let entry = state.entries.get(thread).filter(|entry| entry.turn == turn)?;
		Some((
			entry.error.clone(),
			LiveReviewGuard { reviews: self.clone(), thread: thread.into(), serial: entry.serial },
		))
	}

	pub(super) fn observe(&self, event: &ServerEvent) -> Result<(), ClientError> {
		let ServerEvent::Notification { method, params } = event else {
			return Ok(());
		};
		let Some(thread) =
			params["threadId"].as_str().filter(|id| !id.is_empty() && id.len() <= 512)
		else {
			return Ok(());
		};
		let mut state = self.0.lock().map_err(|_| ClientError::Closed)?;
		if ["turn/started", "thread/reverted", "thread/closed", "thread/archived", "thread/deleted"]
			.contains(&method.as_str())
			|| super::invalidates_question_state(method, params)
		{
			state.entries.remove(thread);
			return Ok(());
		}
		let (turn, error) = match method.as_str() {
			"error" if params["willRetry"] == false => (&params["turnId"], &params["error"]),
			"turn/completed" => (&params["turn"]["id"], &params["turn"]["error"]),
			_ => return Ok(()),
		};
		let Some(turn) = turn.as_str().filter(|id| !id.is_empty() && id.len() <= 512) else {
			return Ok(());
		};
		if error["codexErrorInfo"] != "misalignmentPolicyViolation" {
			state.entries.remove(thread);
			return Ok(());
		}
		// A terminal event may omit the details already supplied by its live error event.
		if state.entries.get(thread).is_some_and(|entry| {
			entry.turn == turn && (entry.error == *error || error["misalignment"].is_null())
		}) {
			return Ok(());
		}
		state.entries.remove(thread);
		if !error["misalignment"].is_object()
			|| error.to_string().len() > 128 * 1024
			|| state.entries.len() >= 32
		{
			return Ok(());
		}
		state.next = state.next.checked_add(1).ok_or(ClientError::CapacityExceeded)?;
		let serial = state.next;
		state
			.entries
			.insert(thread.into(), Entry { turn: turn.into(), serial, error: error.clone() });
		Ok(())
	}
}
