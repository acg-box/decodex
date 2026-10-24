//! Reviewer-only publication. This never changes approval policy or execution permissions.
use super::{AppServerClient, ClientError, HistoryGuard};
use serde_json::Value;
impl AppServerClient {
	/// Publish the review preference for subsequent turns of one exact source.
	/// Success acknowledges publication; the caller must inspect native effective readback.
	pub async fn select_thread_reviewer(
		&self,
		thread: &str,
		reviewer: &str,
		guard: HistoryGuard,
	) -> Result<(), ClientError> {
		let params = serde_json::json!({"threadId":thread,"approvalsReviewer":reviewer});
		if !is_thread_reviewer_selection(&params) {
			return Err(ClientError::InvalidFrame);
		}
		let result = tokio::time::timeout(
			std::time::Duration::from_secs(8),
			self.request_with_history("thread/settings/update", params, guard),
		)
		.await
		.map_err(|_| ClientError::Io)??;
		if result.as_object().is_none_or(|v| !v.is_empty()) {
			return Err(ClientError::InvalidFrame);
		}
		Ok(())
	}
}
/// Only an exact reviewer-only edit is admitted through the retained bridge.
pub fn is_thread_reviewer_selection(params: &Value) -> bool {
	params.as_object().is_some_and(|v| v.len() == 2)
		&& params["threadId"].as_str().is_some_and(|id| {
			!id.is_empty() && id.len() <= 512 && !id.chars().any(char::is_control)
		})
		&& matches!(params["approvalsReviewer"].as_str(), Some("user" | "auto_review"))
}
