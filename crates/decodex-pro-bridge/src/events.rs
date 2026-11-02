//! Completion hints over the existing private native-host connection.
//! The saved receipt remains authoritative after a lost or repeated notification.
use std::{path::Path, time::Duration};

use futures_util::{SinkExt as _, StreamExt as _};
use serde_json::Value;
use tokio::{net::UnixStream, time};
use tokio_tungstenite::{WebSocketStream, tungstenite::Message};

use crate::{Result, native_host};

pub(crate) const SUBSCRIBE: &str = "Decodex.research.subscribe";
pub(crate) const PUBLISH: &str = "Decodex.research.publish";
pub(crate) const UPDATED: &str = "Decodex.research.updated";

/// An observer of saved completions. This connection cannot send a research question.
pub struct Completions(WebSocketStream<UnixStream>);
impl Completions {
	/// Subscribe before reconciling saved IDs, so completion cannot fall between the two.
	pub async fn connect(root: &Path) -> Result<Self> {
		let mut stream = socket(root).await?;

		stream
			.send(Message::Text(serde_json::json!({"id":1,"method":SUBSCRIBE}).to_string().into()))
			.await
			.map_err(|e| e.to_string())?;

		loop {
			let value = read(&mut stream).await?.ok_or("Research host disconnected")?;

			if value["id"] == 1 {
				if value.get("error").is_some() {
					return Err("Research completion subscription was rejected".into());
				}

				return Ok(Self(stream));
			}
		}
	}

	/// Wait locally without model calls. Reconnect and reconcile saved IDs after EOF.
	pub async fn recv(&mut self) -> Result<Option<String>> {
		while let Some(value) = read(&mut self.0).await? {
			if value["method"] == UPDATED {
				return value["params"]["id"]
					.as_str()
					.map(|id| Some(id.into()))
					.ok_or_else(|| "Invalid research completion".into());
			}
		}

		Ok(None)
	}
}

pub(crate) async fn publish(root: &Path, id: &str) {
	// A missing host must not turn a saved successful answer into a failure.
	let _ = time::timeout(Duration::from_secs(2), async {
		let mut stream = socket(root).await?;

		stream
			.send(Message::Text(
				serde_json::json!({"id":1,"method":PUBLISH,"params":{"id":id}}).to_string().into(),
			))
			.await
			.map_err(|e| e.to_string())?;

		while let Some(value) = read(&mut stream).await? {
			if value["id"] == 1 {
				return Ok::<_, String>(());
			}
		}

		Ok(())
	})
	.await;
}

async fn socket(root: &Path) -> Result<WebSocketStream<UnixStream>> {
	let stream =
		UnixStream::connect(native_host::socket_path(root)).await.map_err(|e| e.to_string())?;

	tokio_tungstenite::client_async("ws://localhost/research-events", stream)
		.await
		.map(|(socket, _)| socket)
		.map_err(|e| e.to_string())
}

async fn read(stream: &mut WebSocketStream<UnixStream>) -> Result<Option<Value>> {
	while let Some(message) = stream.next().await {
		match message.map_err(|e| e.to_string())? {
			Message::Text(text) =>
				return serde_json::from_str(&text).map(Some).map_err(|e| e.to_string()),
			Message::Close(_) => return Ok(None),
			_ => {},
		}
	}

	Ok(None)
}
