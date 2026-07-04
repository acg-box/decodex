use std::time::Duration;

use color_eyre::eyre;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

use crate::{
	agent::json_rpc::{
		connection::JsonRpcConnection,
		wire::{JsonRpcMessage, JsonRpcRequest, WireMessage},
	},
	prelude::Result,
};

impl JsonRpcConnection {
	pub(crate) fn request_with_handler<P, T, F>(
		&mut self,
		method: &str,
		params: &P,
		timeout: Duration,
		mut handle_request: F,
	) -> Result<T>
	where
		P: Serialize,
		T: DeserializeOwned,
		F: FnMut(&mut Self, &WireMessage, &JsonRpcRequest) -> Result<()>,
	{
		let request_id = self.next_request_id;
		let expected_id = Value::from(request_id);

		self.next_request_id += 1;

		self.send_value(&serde_json::json!({
			"id": request_id,
			"method": method,
			"params": params,
		}))?;

		loop {
			let wire_message = self.read_message(Some(timeout))?;

			match &wire_message.message {
				JsonRpcMessage::Notification(_) => self.pending_messages.push_back(wire_message),
				JsonRpcMessage::Response(response) if response.id == expected_id => {
					return Ok(serde_json::from_value(response.result.clone())?);
				},
				JsonRpcMessage::Error(error) if error.id == expected_id => {
					let data = error
						.error
						.data
						.as_ref()
						.map_or_else(String::new, |data| format!(" data: {data}"));

					return Err(eyre::eyre!(
						"`{method}` failed with {}: {}{}",
						error.error.code,
						error.error.message,
						data
					));
				},
				JsonRpcMessage::Request(request) => handle_request(self, &wire_message, request)?,
				JsonRpcMessage::Response(response) => {
					tracing::debug!(
						method,
						response_id = %response.id,
						expected_id = %expected_id,
						"Recorded and ignored orphan app-server JSON-RPC response while waiting for request."
					);
				},
				JsonRpcMessage::Error(error) => {
					return Err(eyre::eyre!(
						"Received an unexpected JSON-RPC error while waiting for `{method}`: id {} failed with {}: {}",
						error.id,
						error.error.code,
						error.error.message
					));
				},
			}
		}
	}

	pub(crate) fn notify<P>(&mut self, method: &str, params: Option<&P>) -> Result<()>
	where
		P: Serialize,
	{
		let value = match params {
			Some(params) => serde_json::json!({
				"method": method,
				"params": params,
			}),
			None => serde_json::json!({ "method": method }),
		};

		self.send_value(&value)
	}

	pub(crate) fn recv(&mut self, timeout: Option<Duration>) -> Result<WireMessage> {
		if let Some(message) = self.pending_messages.pop_front() {
			return Ok(message);
		}

		self.read_message(timeout)
	}

	pub(crate) fn respond<R>(&mut self, id: &Value, result: &R) -> Result<()>
	where
		R: Serialize,
	{
		self.send_value(&serde_json::json!({
			"id": id,
			"result": result,
		}))
	}

	pub(crate) fn respond_error(&mut self, id: &Value, code: i64, message: &str) -> Result<()> {
		self.send_value(&serde_json::json!({
			"id": id,
			"error": {
				"code": code,
				"message": message,
			},
		}))
	}

	pub(crate) fn drain_pending(&mut self) -> Vec<WireMessage> {
		self.pending_messages.drain(..).collect()
	}
}
