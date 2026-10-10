//! Explicit ephemeral prediction forks. These inherit native tools and permissions.
use super::{AppServerClient, ClientError, HistoryGuard, ServerEvent, TemporaryStructuredThread};
use serde_json::Value;
use std::time::Duration;
use tokio::{
	sync::{mpsc, watch},
	time,
};

/// A context-preserving temporary branch. It is not a tool-free recap thread.
/// The caller owns authorization for inherited tools and must route this thread's events.
#[must_use = "Run or cancel the prediction and await native cleanup"]
pub struct NativePredictionThread(TemporaryStructuredThread);
impl NativePredictionThread {
	/// Exact identity for the caller's temporary event route.
	pub fn id(&self) -> &str {
		self.0.id()
	}

	/// Detach a prediction before inference, without changing the parent.
	pub async fn cancel(self) -> Result<(), ClientError> {
		self.0.cancel().await
	}

	/// Run once with inherited settings and a 30-second deadline, then detach.
	/// Cancellation interrupts the exact returned turn. This never submits to the parent,
	/// edits a draft, retries inference, or acknowledges a suggestion on the user's behalf.
	pub async fn run(
		self,
		prompt: String,
		output_schema: Value,
		events: mpsc::Receiver<ServerEvent>,
		cancellation: watch::Receiver<bool>,
	) -> Result<String, ClientError> {
		self.0.run(prompt, output_schema, None, events, cancellation).await
	}
}
/// Admit only ephemeral whole-context prediction forks with no configuration overrides.
pub fn is_prediction_fork(params: &Value) -> bool {
	params.as_object().is_some_and(|p| p.len() == 4)
		&& params["threadId"].as_str().is_some_and(|id| {
			!id.is_empty() && id.len() <= 512 && !id.chars().any(char::is_control)
		})
		&& params["experimentalPredictionMode"] == true
		&& params["ephemeral"] == true
		&& params["excludeTurns"] == true
}
impl AppServerClient {
	/// Fork a loaded, persisted parent once. The native server inherits its model, tools,
	/// provider, environment, and permissions. No background prediction is scheduled.
	/// Await the result even if cancelled, then cancel the returned branch before inference.
	pub async fn fork_prediction_thread(
		&self,
		source: &str,
		guard: HistoryGuard,
	) -> Result<NativePredictionThread, ClientError> {
		let params = serde_json::json!({"threadId":source,"experimentalPredictionMode":true,"ephemeral":true,"excludeTurns":true});
		if !is_prediction_fork(&params) {
			return Err(ClientError::InvalidFrame);
		}
		let guard =
			self.with_thread_settings_guard(source, guard).ok_or(ClientError::StaleHistory)?;
		let result = time::timeout(
			Duration::from_secs(30),
			self.request_with_history("thread/fork", params, guard),
		)
		.await
		.map_err(|_| ClientError::Io)??;
		let id = result["thread"]["id"]
			.as_str()
			.filter(|id| {
				!id.is_empty()
					&& id.len() <= 512
					&& *id != source
					&& !id.chars().any(char::is_control)
			})
			.ok_or(ClientError::InvalidFrame)?;
		if result["thread"]["forkedFromId"] != source || !result["thread"]["path"].is_null() {
			return Err(ClientError::InvalidFrame);
		}
		Ok(NativePredictionThread(TemporaryStructuredThread::from_ephemeral(
			self.clone(),
			id.to_owned(),
		)))
	}
}
#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn prediction_forks_reject_persistence_cutoffs_and_all_overrides() {
		let params = serde_json::json!({"threadId":"parent","experimentalPredictionMode":true,"ephemeral":true,"excludeTurns":true});
		assert!(is_prediction_fork(&params));
		for key in [
			"model",
			"modelProvider",
			"config",
			"beforeTurnId",
			"lastTurnId",
			"path",
			"permissions",
			"deferGoalContinuation",
		] {
			let mut invalid = params.clone();
			invalid[key] = Value::Null;
			assert!(!is_prediction_fork(&invalid));
		}
		for key in ["ephemeral", "excludeTurns", "experimentalPredictionMode"] {
			let mut invalid = params.clone();
			invalid[key] = Value::Bool(false);
			assert!(!is_prediction_fork(&invalid));
		}
	}
}
