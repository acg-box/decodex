//! Transport-observed request liveness, independent of an owner's event queue.
use super::{ClientError, RequestId, ServerEvent};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use std::{
	collections::HashMap,
	sync::{
		Arc, Mutex,
		atomic::{AtomicU64, Ordering},
	},
};

#[derive(Clone, Default)]
pub(super) struct ServerRequests(
	Arc<Mutex<HashMap<RequestId, Entry>>>,
	Arc<AtomicU64>,
	Arc<AtomicU64>,
	super::live_reviews::LiveReviews,
);
struct Entry {
	thread: String,
	turn: Option<String>,
	digest: [u8; 32],
}

/// Exact history, question state, or live continuation evidence on one native connection.
#[derive(Clone)]
pub struct HistoryGuard {
	requests: ServerRequests,
	revision: u64,
	questions: bool,
	review: Option<super::live_reviews::LiveReviewGuard>,
}
impl HistoryGuard {
	/// Identity of the exact live review, absent for history guards or invalidated evidence.
	pub fn live_review_identity(&self) -> Option<String> {
		self.review.as_ref()?.identity()
	}

	pub(super) fn belongs_to(&self, requests: &ServerRequests) -> bool {
		Arc::ptr_eq(&self.requests.0, &requests.0)
	}

	/// Whether the selected native evidence is still current on its source connection.
	pub fn is_live(&self) -> bool {
		if let Some(review) = &self.review {
			return review.is_live();
		}
		(if self.questions {
			self.requests.question_revision()
		} else {
			self.requests.history_revision()
		}) == self.revision
	}
}

/// Exact live request observed on this transport, not transferable between connections.
#[derive(Clone)]
pub struct ServerRequestGuard {
	requests: ServerRequests,
	id: RequestId,
	digest: [u8; 32],
}
impl ServerRequestGuard {
	pub(super) fn matches_id(&self, id: &RequestId) -> bool {
		self.id == *id
	}

	pub(super) fn belongs_to(&self, requests: &ServerRequests) -> bool {
		Arc::ptr_eq(&self.requests.0, &requests.0)
	}

	/// Whether the transport still observes this exact request as unresolved.
	pub fn is_live(&self) -> bool {
		self.requests
			.0
			.lock()
			.is_ok_and(|rows| rows.get(&self.id).is_some_and(|e| e.digest == self.digest))
	}
}

fn digest(method: &str, params: &Value) -> [u8; 32] {
	Sha256::digest(json!([method, params]).to_string().as_bytes()).into()
}
impl ServerRequests {
	pub(super) fn live_misalignment_review(
		&self,
		thread: &str,
		turn: &str,
	) -> Option<(Value, HistoryGuard)> {
		let (error, review) = self.3.capture(thread, turn)?;
		Some((
			error,
			HistoryGuard {
				requests: self.clone(),
				revision: 0,
				questions: false,
				review: Some(review),
			},
		))
	}

	pub(super) fn history_guard(&self, revision: u64) -> Option<HistoryGuard> {
		(self.history_revision() == revision).then(|| HistoryGuard {
			requests: self.clone(),
			revision,
			questions: false,
			review: None,
		})
	}

	pub(super) fn question_guard(&self, revision: u64) -> Option<HistoryGuard> {
		(self.question_revision() == revision).then(|| HistoryGuard {
			requests: self.clone(),
			revision,
			questions: true,
			review: None,
		})
	}

	pub(super) fn question_revision(&self) -> u64 {
		self.2.load(Ordering::Acquire)
	}

	pub(super) fn history_revision(&self) -> u64 {
		self.1.load(Ordering::Acquire)
	}

	pub(super) fn guard(
		&self,
		id: &RequestId,
		method: &str,
		params: &Value,
	) -> Option<ServerRequestGuard> {
		let guard = ServerRequestGuard {
			requests: self.clone(),
			id: id.clone(),
			digest: digest(method, params),
		};
		guard.is_live().then_some(guard)
	}

	pub(super) fn remove(&self, id: &RequestId) {
		if let Ok(mut rows) = self.0.lock() {
			rows.remove(id);
		}
	}

	pub(super) fn clear(&self) {
		self.3.clear();
		if let Ok(mut rows) = self.0.lock() {
			rows.clear();
		}
	}

	pub(super) fn observe(&self, event: &ServerEvent) -> Result<(), ClientError> {
		self.3.observe(event)?;
		let mut rows = self.0.lock().map_err(|_| ClientError::Closed)?;
		if let ServerEvent::Notification { method, params } = event
			&& invalidates_question_state(method, params)
		{
			self.2.fetch_add(1, Ordering::AcqRel);
		}
		if let ServerEvent::Notification { method, params } = event
			&& method == "thread/reverted"
			&& params["threadId"].as_str().is_some_and(|id| !id.is_empty())
		{
			self.1.fetch_add(1, Ordering::AcqRel);
		}
		match event {
			ServerEvent::Request { id, method, params } => {
				if let Some(thread) = params["threadId"].as_str() {
					if rows.len() >= 256 {
						return Err(ClientError::CapacityExceeded);
					}
					if rows.contains_key(id) {
						return Err(ClientError::InvalidFrame);
					}
					rows.insert(
						id.clone(),
						Entry {
							thread: thread.into(),
							turn: params["turnId"].as_str().map(str::to_owned),
							digest: digest(method, params),
						},
					);
				}
			},
			ServerEvent::Notification { method, params } if method == "serverRequest/resolved" =>
				if let (Some(thread), Ok(id)) = (
					params["threadId"].as_str(),
					serde_json::from_value::<RequestId>(params["requestId"].clone()),
				) && rows.get(&id).is_some_and(|e| e.thread == thread)
				{
					rows.remove(&id);
				},
			ServerEvent::Notification { method, params }
				if ["thread/closed", "thread/archived", "thread/deleted", "thread/reverted"]
					.contains(&method.as_str()) =>
				if let Some(thread) = params["threadId"].as_str() {
					rows.retain(|_, e| e.thread != thread);
				},
			ServerEvent::Notification { method, params } if method == "turn/completed" => {
				if let (Some(thread), Some(turn)) =
					(params["threadId"].as_str(), params["turn"]["id"].as_str())
				{
					rows.retain(|_, e| e.thread != thread || e.turn.as_deref() != Some(turn));
				}
			},
			_ => {},
		}
		Ok(())
	}
}

/// Whether a committed native input or revert can invalidate a pending question.
pub fn invalidates_question_state(method: &str, params: &Value) -> bool {
	params["threadId"].as_str().is_some_and(|id| !id.is_empty())
		&& (method == "thread/reverted"
			|| (method == "item/completed"
				&& params["item"]["type"] == "userMessage"
				&& params["turnId"].as_str().is_some_and(|id| !id.is_empty())
				&& params["item"]["id"].as_str().is_some_and(|id| !id.is_empty())))
}
