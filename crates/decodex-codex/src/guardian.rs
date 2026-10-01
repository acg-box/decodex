//! Observed native Guardian reviews. An assessment is not an execution receipt.
//!
//! Keep the full public event: the native approval endpoint takes a different
//! core event format, and dropping unknown action fields could change what the
//! user approves. Decoding this observation never authorizes that conversion.

mod approval;

pub use approval::core_denial_event;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use decodex_core::MAX_NATIVE_MESSAGE_BYTES;

/// Maximum retained public review, including its action and explanation.
pub const MAX_REVIEW_BYTES: usize = MAX_NATIVE_MESSAGE_BYTES;

/// Native review lifecycle, independent of the reviewed command's lifecycle.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ReviewStatus {
	/// No final assessment has been observed.
	InProgress,
	/// The reviewer allowed the action; this does not prove execution.
	Approved,
	/// The reviewer denied the action.
	Denied,
	/// The reviewer did not finish before its deadline.
	TimedOut,
	/// The review was cancelled.
	Aborted,
}

/// Validated identity and lifecycle plus the original public event.
#[derive(Clone, Debug, PartialEq)]
pub struct GuardianReview {
	/// Native thread that emitted the observation.
	pub thread_id: String,
	/// Exact native turn that owns the review.
	pub turn_id: String,
	/// Review identity; several reviews can share one target item.
	pub review_id: String,
	/// Reviewed item, absent for network reviews.
	pub target_item_id: Option<String>,
	/// Unix milliseconds reported by the provider.
	pub started_at_ms: i64,
	/// Present only for a completed notification.
	pub completed_at_ms: Option<i64>,
	/// Assessment state, not command state.
	pub status: ReviewStatus,
	/// Full observation, including unknown fields, for exact durable readback.
	pub event: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Envelope {
	thread_id: String,
	turn_id: String,
	review_id: String,
	target_item_id: Option<String>,
	started_at_ms: i64,
	completed_at_ms: Option<i64>,
	review: Assessment,
	action: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Assessment {
	status: ReviewStatus,
	risk_level: Option<String>,
	user_authorization: Option<String>,
	#[serde(rename = "rationale")]
	_rationale: Option<String>,
}

/// Decode one lifecycle notification. Unknown action variants are retained as
/// observations so future native reviews do not disappear from history. They
/// must not become user-approval requests without a supported exact converter.
pub fn decode_review(method: &str, params: &Value) -> Option<GuardianReview> {
	let completed = match method {
		"item/autoApprovalReview/started" => false,
		"item/autoApprovalReview/completed" => true,
		_ => return None,
	};

	if serde_json::to_vec(params).ok()?.len() > MAX_REVIEW_BYTES {
		return None;
	}

	let event: Envelope = serde_json::from_value(params.clone()).ok()?;
	let valid_id = |id: &str| !id.trim().is_empty() && id.len() <= 512;

	if !valid_id(&event.thread_id)
		|| !valid_id(&event.turn_id)
		|| !valid_id(&event.review_id)
		|| event.target_item_id.as_deref().is_some_and(|id| !valid_id(id))
		|| event.started_at_ms < 0
		|| completed != (event.review.status != ReviewStatus::InProgress)
		|| completed != event.completed_at_ms.is_some()
		|| event.completed_at_ms.is_some_and(|time| time < 0)
		|| event.action.get("type").and_then(Value::as_str).is_none_or(|kind| !valid_id(kind))
		|| event
			.review
			.risk_level
			.as_deref()
			.is_some_and(|risk| !["low", "medium", "high", "critical"].contains(&risk))
		|| event
			.review
			.user_authorization
			.as_deref()
			.is_some_and(|level| !["unknown", "low", "medium", "high"].contains(&level))
		|| (completed && params["decisionSource"].as_str() != Some("agent"))
	{
		return None;
	}
	// Do not compare wall-clock timestamps: the system clock can move during a review.
	Some(GuardianReview {
		thread_id: event.thread_id,
		turn_id: event.turn_id,
		review_id: event.review_id,
		target_item_id: event.target_item_id,
		started_at_ms: event.started_at_ms,
		completed_at_ms: event.completed_at_ms,
		status: event.review.status,
		event: params.clone(),
	})
}

#[cfg(test)]
mod tests {
	use serde_json;

	use crate::guardian::{self, MAX_REVIEW_BYTES, Value};
	const COMPLETED: &str = "item/autoApprovalReview/completed";
	const STARTED: &str = "item/autoApprovalReview/started";

	fn denial() -> Value {
		serde_json::json!({"threadId":"thread", "turnId":"turn", "reviewId":"review",
			"targetItemId":null,"startedAtMs":200,"completedAtMs":201,
			"decisionSource":"agent",
			"review":{"status":"denied","riskLevel":"high",
				"userAuthorization":"low","rationale":"This host was not requested."},
			"action":{"type":"networkAccess","target":"https://example.test:443",
				"host":"example.test","protocol":"https","port":443}})
	}

	#[test]
	fn retains_complete_large_native_action_and_denial_conversion() {
		let command = "界".repeat(100_000) + " exact-required-suffix";
		let mut event = denial();

		event["action"] =
			serde_json::json!({"type":"command","source":"shell","command":command,"cwd":"/tmp"});
		event["review"]["rationale"] = serde_json::json!("Complete findings. ".repeat(6_000));

		assert!(event.to_string().len() > 256 * 1_024);
		assert!(event.to_string().len() < decodex_core::MAX_NATIVE_MESSAGE_BYTES);

		let observed = guardian::decode_review(COMPLETED, &event).expect("complete native review");

		assert_eq!(observed.event, event);

		let converted =
			guardian::core_denial_event(&observed).expect("complete supported command action");

		assert_eq!(converted["action"]["command"], command);
	}

	#[test]
	fn retains_network_without_target_and_distinct_reviews_for_one_item() {
		let mut event = denial();
		let first = guardian::decode_review(COMPLETED, &event).unwrap();

		assert_eq!(first.target_item_id, None);
		assert_eq!(first.event, event);

		event["targetItemId"] = serde_json::json!("command");
		event["reviewId"] = serde_json::json!("execve-1");

		let first = guardian::decode_review(COMPLETED, &event).unwrap();

		event["reviewId"] = serde_json::json!("execve-2");

		let second = guardian::decode_review(COMPLETED, &event).unwrap();

		assert_eq!(first.target_item_id, second.target_item_id);
		assert_ne!(first.review_id, second.review_id);
	}

	#[test]
	fn validates_lifecycle_without_inferring_success_or_clock_order() {
		for status in ["approved", "denied", "timedOut", "aborted"] {
			let mut event = denial();

			event["review"]["status"] = serde_json::json!(status);
			event["completedAtMs"] = serde_json::json!(199);

			assert!(guardian::decode_review(COMPLETED, &event).is_some());
			assert!(guardian::decode_review(STARTED, &event).is_none());
		}

		let mut event = denial();

		event["review"]["status"] = serde_json::json!("inProgress");

		event.as_object_mut().unwrap().remove("completedAtMs");
		event.as_object_mut().unwrap().remove("decisionSource");

		assert!(guardian::decode_review(STARTED, &event).is_some());
		assert!(guardian::decode_review(COMPLETED, &event).is_none());
		assert!(
			guardian::decode_review("autoApprovalReview/strictReviewRequired", &event).is_none()
		);
	}

	#[test]
	fn rejects_incomplete_malformed_and_oversized_observations() {
		for field in [
			"threadId",
			"turnId",
			"reviewId",
			"startedAtMs",
			"completedAtMs",
			"decisionSource",
			"review",
			"action",
		] {
			let mut event = denial();

			event.as_object_mut().unwrap().remove(field);

			assert!(guardian::decode_review(COMPLETED, &event).is_none(), "{field}");
		}
		for (pointer, value) in [
			("/threadId", serde_json::json!(" ")),
			("/turnId", serde_json::json!("x".repeat(513))),
			("/startedAtMs", serde_json::json!(-1)),
			("/completedAtMs", serde_json::json!(-1)),
			("/review/status", serde_json::json!("unknown")),
			("/review/riskLevel", serde_json::json!("safe")),
			("/review/userAuthorization", serde_json::json!("approved")),
			("/action", serde_json::json!([])),
			("/action/type", serde_json::json!(null)),
		] {
			let mut event = denial();

			*event.pointer_mut(pointer).unwrap() = value;

			assert!(guardian::decode_review(COMPLETED, &event).is_none(), "{pointer}");
		}

		let mut event = denial();

		event["future"] = serde_json::json!("x".repeat(MAX_REVIEW_BYTES));

		assert!(guardian::decode_review(COMPLETED, &event).is_none());
	}

	#[test]
	fn preserves_unknown_action_and_fields_without_authorizing_them() {
		let mut event = denial();

		event["action"] =
			serde_json::json!({"type":"futureAction","payload":{"camelCase":"exact"}});
		event["futureAttribution"] = serde_json::json!({"plugin":"native"});

		assert_eq!(guardian::decode_review(COMPLETED, &event).unwrap().event, event);
	}
}
