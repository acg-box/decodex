//! Optional task recap state. Querying never starts inference.
use crate::{EntityId, WireText};
use serde::{Deserialize, Serialize};

/// A validated plain-text recap from the isolated native request.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TaskRecap {
	/// Broader goal, meaningful progress, and remaining caveats.
	pub summary: WireText,
	/// Explicit outstanding user question or agreed next step, if any.
	pub next_action: Option<WireText>,
}
impl TaskRecap {
	/// Enforce the native recap's Unicode character limits.
	pub fn is_valid(&self) -> bool {
		!self.summary.as_str().trim().is_empty()
			&& self.summary.as_str().chars().count() <= 700
			&& self
				.next_action
				.as_ref()
				.is_none_or(|v| !v.as_str().trim().is_empty() && v.as_str().chars().count() <= 200)
	}
}

/// Lifecycle phase of a service-owned optional recap request.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskRecapPhase {
	/// No retained request exists for this task.
	Idle,
	/// History or isolated inference is in progress.
	Pending,
	/// Cancellation was requested; the native cleanup attempt is still in progress.
	Cancelling,
	/// A current completed result is available.
	Ready,
	/// The request failed or cleanup was not confirmed.
	Failed,
	/// The local recap was cancelled or invalidated; no result will be displayed.
	Cancelled,
}

/// State for one exact task; explicit generation commands alone start inference.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TaskRecapStatus {
	/// Local owning task.
	pub work_id: EntityId,
	/// Native thread selected when the request was accepted.
	pub thread_id: Option<WireText>,
	/// Original command identity; late results never replace another request.
	pub request_id: Option<WireText>,
	/// Current lifecycle phase.
	pub phase: TaskRecapPhase,
	/// Available only after successful current-source completion.
	pub recap: Option<TaskRecap>,
}
impl TaskRecapStatus {
	/// Check phase and identity consistency before presentation.
	pub fn is_valid(&self) -> bool {
		self.request_id.as_ref().is_none_or(|id| !id.as_str().is_empty())
			&& self.thread_id.as_ref().is_none_or(|id| !id.as_str().is_empty())
			&& (self.phase == TaskRecapPhase::Idle)
				== (self.request_id.is_none() && self.thread_id.is_none())
			&& (self.phase == TaskRecapPhase::Idle
				|| (self.request_id.is_some() && self.thread_id.is_some()))
			&& match (&self.phase, &self.recap) {
				(TaskRecapPhase::Ready, Some(recap)) => recap.is_valid(),
				(TaskRecapPhase::Ready, None) | (_, Some(_)) => false,
				_ => true,
			}
	}
}
