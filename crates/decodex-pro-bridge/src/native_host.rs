//! Chrome-owned Native Messaging host. Local workers share one extension connection.
//! No HTTP port, browser launch, official desktop process, or model inference is used.

use std::{
	collections::HashMap,
	path::{Path, PathBuf},
	sync::{
		Arc, Mutex,
		atomic::{AtomicU64, Ordering},
	},
	time::Duration,
};

use futures_util::{SinkExt as _, StreamExt as _};
use serde_json::{Value, json};
use tokio::{
	io::{AsyncRead, AsyncWrite},
	net::{UnixListener, UnixStream},
	sync::{broadcast, mpsc, oneshot},
};
use tokio_tungstenite::{accept_async, tungstenite::Message};

use crate::{Result, native_wire, store::Store};

/// Chrome Native Messaging registration name.
pub const HOST_NAME: &str = "box.acg.decodex.research";
/// Stable ID derived from the extension manifest's public key.
pub const EXTENSION_ID: &str = "appokabijkobnaennkffkjldbikangai";

pub(crate) fn socket_path(root: &Path) -> PathBuf {
	root.join("extension.sock")
}

/// Run only for the registered extension origin. Chrome supplies this argument.
pub async fn serve(root: &Path, origin: &str) -> Result<()> {
	if origin != format!("chrome-extension://{EXTENSION_ID}/") {
		return Err("The native host only accepts the Decodex Research extension.".into());
	}
	serve_io(root, tokio::io::stdin(), tokio::io::stdout()).await
}

async fn serve_io(
	root: &Path,
	read: impl AsyncRead + Unpin + Send + 'static,
	write: impl AsyncWrite + Unpin + Send + 'static,
) -> Result<()> {
	let store = Store::open(root)?;
	let _lease = store.browser_lease()?.ok_or("Another research host is connected.")?;
	let path = socket_path(root);
	match std::fs::remove_file(&path) {
		Ok(()) => {},
		Err(e) if e.kind() == std::io::ErrorKind::NotFound => {},
		Err(e) => return Err(e.to_string()),
	}
	let listener = UnixListener::bind(&path).map_err(|e| e.to_string())?;
	let _socket = SocketGuard(path);
	let (outbound, mut outgoing) = mpsc::channel::<Value>(32);
	let (events, _) = broadcast::channel(1024);
	let hub = Arc::new(Hub {
		outbound,
		pending: Mutex::new(HashMap::new()),
		events,
		next: AtomicU64::new(1),
	});
	let mut writer = write;
	let write_loop = async move {
		while let Some(value) = outgoing.recv().await {
			native_wire::write_packet(&mut writer, &value).await.map_err(|e| e.to_string())?;
		}
		Ok(())
	};
	let mut reader = read;
	let input_hub = hub.clone();
	let read_loop = async move {
		loop {
			let value = native_wire::read_packet(&mut reader).await.map_err(|e| e.to_string())?;
			if let Some(id) = value["id"].as_u64() {
				if let Some(reply) =
					input_hub.pending.lock().expect("pending native requests").remove(&id)
				{
					let _ = reply.send(value);
				}
			} else {
				let _ = input_hub.events.send(value);
			}
		}
		#[allow(unreachable_code)]
		Ok(())
	};
	let clients = async {
		let mut clients = tokio::task::JoinSet::new();
		loop {
			tokio::select! {
				connection = listener.accept() => {
					let (stream, _) = connection.map_err(|e| e.to_string())?;
					clients.spawn(serve_client(stream, hub.clone()));
				},
				_ = clients.join_next(), if !clients.is_empty() => {},
			}
		}
		#[allow(unreachable_code)]
		Ok(())
	};
	// Extension EOF ends the host and all local observers. Saved work is not resent.
	tokio::select! {
		result = read_loop => result,
		result = write_loop => result,
		result = clients => result,
	}
}

struct SocketGuard(PathBuf);
impl Drop for SocketGuard {
	fn drop(&mut self) {
		let _ = std::fs::remove_file(&self.0);
	}
}

struct Hub {
	outbound: mpsc::Sender<Value>,
	pending: Mutex<HashMap<u64, oneshot::Sender<Value>>>,
	events: broadcast::Sender<Value>,
	next: AtomicU64,
}

struct Pending<'a>(&'a Hub, u64);
impl Drop for Pending<'_> {
	fn drop(&mut self) {
		self.0.pending.lock().expect("pending native requests").remove(&self.1);
	}
}

impl Hub {
	async fn request(&self, mut value: Value) -> Value {
		let original = value["id"].clone();
		let id = self.next.fetch_add(1, Ordering::Relaxed);
		value["id"] = json!(id);
		let (send, reply) = oneshot::channel();
		self.pending.lock().expect("pending native requests").insert(id, send);
		let _pending = Pending(self, id);
		let result = async {
			self.outbound.send(value).await.map_err(|_| "Research extension disconnected")?;
			tokio::time::timeout(Duration::from_secs(30), reply)
				.await
				.map_err(
					|_| "Research extension command timed out; inspect the saved request before retrying",
				)?
				.map_err(|_| "Research extension disconnected")
		}
		.await;
		let mut result = result.unwrap_or_else(|message| json!({"error":{"message":message}}));
		result["id"] = original;
		result
	}
}

async fn serve_client(stream: UnixStream, hub: Arc<Hub>) -> Result<()> {
	let socket = accept_async(stream).await.map_err(|e| e.to_string())?;
	let (mut write, mut read) = socket.split();
	let mut events = hub.events.subscribe();
	let (reply, mut replies) = mpsc::channel::<Value>(16);
	let mut requests = tokio::task::JoinSet::new();
	loop {
		tokio::select! {
			message = read.next() => {
				match message {
					Some(Ok(Message::Text(text))) => {
						let value: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
						let (hub, reply) = (hub.clone(), reply.clone());
						requests.spawn(async move { let _ = reply.send(hub.request(value).await).await; });
					},
					Some(Ok(Message::Ping(bytes))) => write.send(Message::Pong(bytes)).await.map_err(|e| e.to_string())?,
					Some(Ok(Message::Close(_))) | None => return Ok(()),
					Some(Err(error)) => return Err(error.to_string()),
					_ => {},
				}
			},
			Some(reply) = replies.recv() => write.send(Message::Text(reply.to_string().into())).await.map_err(|e| e.to_string())?,
			event = events.recv() => {
				let value = event.map_err(|_| "Research events were lost; recover the saved request")?;
				write.send(Message::Text(value.to_string().into())).await.map_err(|e| e.to_string())?;
			},
			_ = requests.join_next(), if !requests.is_empty() => {},
		}
	}
}

#[cfg(test)]
#[path = "native_host_tests.rs"]
mod tests;
