//! Chrome-owned Native Messaging host. Local workers share one extension connection.
//! No HTTP port, browser launch, official desktop process, or model inference is used.

use std::{
	collections::HashMap,
	fs,
	io::ErrorKind,
	os::unix::process,
	path::{Path, PathBuf},
	process::Command,
	sync::{
		Arc, Mutex,
		atomic::{AtomicBool, AtomicU64, Ordering},
	},
	time::Duration,
};

use futures_util::{SinkExt as _, StreamExt as _};
use serde_json::Value;
use tokio::{
	io::{self, AsyncRead, AsyncWrite},
	net::{UnixListener, UnixStream},
	sync::{broadcast, mpsc, oneshot},
	task::JoinSet,
	time,
};
use tokio_tungstenite::tungstenite::Message;

use crate::{
	Phase, Result,
	events::{PUBLISH, SUBSCRIBE},
	native_wire,
	store::Store,
};

/// Chrome Native Messaging registration name.
pub const HOST_NAME: &str = "box.acg.decodex.research";
/// Stable ID derived from the extension manifest's public key.
pub const EXTENSION_ID: &str = "appokabijkobnaennkffkjldbikangai";

struct SocketGuard(PathBuf);
impl Drop for SocketGuard {
	fn drop(&mut self) {
		let _ = fs::remove_file(&self.0);
	}
}

struct Hub {
	outbound: mpsc::Sender<Value>,
	pending: Mutex<HashMap<u64, oneshot::Sender<Value>>>,
	events: broadcast::Sender<Value>,
	next: AtomicU64,
}
impl Hub {
	async fn request(&self, mut value: Value) -> Value {
		let original = value["id"].clone();
		let id = self.next.fetch_add(1, Ordering::Relaxed);

		value["id"] = serde_json::json!(id);

		let (send, reply) = oneshot::channel();

		self.pending.lock().expect("pending native requests").insert(id, send);

		let _pending = Pending(self, id);
		let result = async {
			self.outbound.send(value).await.map_err(|_| "Research extension disconnected")?;

			time::timeout(Duration::from_secs(30), reply)
				.await
				.map_err(
					|_| "Research extension command timed out; inspect the saved request before retrying",
				)?
				.map_err(|_| "Research extension disconnected")
		}
		.await;
		let mut result =
			result.unwrap_or_else(|message| serde_json::json!({"error":{"message":message}}));

		result["id"] = original;

		result
	}
}

struct Pending<'a>(&'a Hub, u64);
impl Drop for Pending<'_> {
	fn drop(&mut self) {
		self.0.pending.lock().expect("pending native requests").remove(&self.1);
	}
}

pub(crate) fn socket_path(root: &Path) -> PathBuf {
	root.join("extension.sock")
}

/// Run only for the registered extension origin. Chrome supplies this argument.
pub async fn serve(root: &Path, origin: &str) -> Result<()> {
	if origin != format!("chrome-extension://{EXTENSION_ID}/") {
		return Err("The native host only accepts the Decodex Pro Research extension.".into());
	}

	serve_io(root, io::stdin(), io::stdout(), browser_session()?).await
}

fn browser_session() -> Result<String> {
	let parent = process::parent_id();
	let output = Command::new("/bin/ps")
		.args(["-p", &parent.to_string(), "-o", "lstart="])
		.env("LC_ALL", "C")
		.output()
		.map_err(|e| e.to_string())?;
	let started = String::from_utf8(output.stdout).map_err(|e| e.to_string())?;

	if !output.status.success() || started.trim().is_empty() {
		return Err("Cannot identify the Chrome process that started this research host.".into());
	}

	Ok(format!("{parent}:{}", started.trim()))
}

// Local result hints never enter Chrome's command channel.
fn research_request(
	root: &Path,
	hub: &Hub,
	request: &Value,
	subscribed: &AtomicBool,
) -> Option<Value> {
	match request["method"].as_str()? {
		SUBSCRIBE => subscribed.store(true, Ordering::Relaxed),
		PUBLISH => {
			let ready = request["params"]["id"]
				.as_str()
				.and_then(|id| Store::open(root).ok()?.load(id).ok())
				.filter(|r| {
					(r.receipt.phase == Phase::Complete && r.answer.is_some())
						|| r.receipt.last_error.is_some()
				});

			if let Some(record) = ready {
				let _ = hub.events.send(
					serde_json::json!({"method":crate::events::UPDATED,"params":{"id":record.receipt.id}}),
				);
			}
		},
		_ => return None,
	}

	Some(serde_json::json!({"id":request["id"],"result":{}}))
}

async fn serve_io(
	root: &Path,
	read: impl AsyncRead + Unpin + Send + 'static,
	write: impl AsyncWrite + Unpin + Send + 'static,
	browser_session: String,
) -> Result<()> {
	let store = Store::open(root)?;
	let _lease = store.browser_lease()?.ok_or("Another research host is connected.")?;
	let path = socket_path(root);

	match fs::remove_file(&path) {
		Ok(()) => {},
		Err(e) if e.kind() == ErrorKind::NotFound => {},
		Err(e) => return Err(e.to_string()),
	}

	let listener = UnixListener::bind(&path).map_err(|e| e.to_string())?;
	let _socket = SocketGuard(path);
	let (outbound, mut outgoing) = mpsc::channel::<Value>(32);
	let (events, _) = broadcast::channel(1_024);
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

			if value["kind"] == "hello" {
				input_hub
					.outbound
					.send(serde_json::json!({"kind":"session","browserSession":browser_session}))
					.await
					.map_err(|e| e.to_string())?;
			} else if let Some(id) = value["id"].as_u64() {
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
		let mut clients = JoinSet::new();

		loop {
			tokio::select! {
				connection = listener.accept() => {
					let (stream, _) = connection.map_err(|e| e.to_string())?;

					clients.spawn(serve_client(stream, hub.clone(), root.to_path_buf()));
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

async fn serve_client(stream: UnixStream, hub: Arc<Hub>, root: PathBuf) -> Result<()> {
	let socket = tokio_tungstenite::accept_async(stream).await.map_err(|e| e.to_string())?;
	let research = Arc::new(AtomicBool::new(false));
	let read_research = research.clone();
	let (reply, mut replies) = mpsc::channel::<Message>(16);
	let (mut write, mut read) = socket.split();
	let mut events = hub.events.subscribe();
	// Read independently of event writes. Both sides can send multi-megabyte
	// packets; waiting for one write before reading the other side deadlocks.
	let read_loop = async move {
		let mut requests = JoinSet::new();

		loop {
			tokio::select! {
				message = read.next() => {
					match message {
						Some(Ok(Message::Text(text))) => {
							let value: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;

							if let Some(result) = research_request(&root,&hub,&value,&read_research) {
								reply.send(Message::Text(result.to_string().into())).await.map_err(|e|e.to_string())?;

								continue;
							}

							let (hub, reply) = (hub.clone(), reply.clone());

							requests.spawn(async move {
								let result = hub.request(value).await;
								let _ = reply.send(Message::Text(result.to_string().into())).await;
							});
						},
						Some(Ok(Message::Ping(bytes))) => reply.send(Message::Pong(bytes)).await.map_err(|e|e.to_string())?,
						Some(Ok(Message::Close(_))) | None => return Ok(()),
						Some(Err(error)) => return Err(error.to_string()),
						_ => {},
					}
				},
				_ = requests.join_next(), if !requests.is_empty() => {},
			}
		}
	};
	let write_loop = async move {
		loop {
			tokio::select! {
				Some(reply) = replies.recv() => write.send(reply).await.map_err(|e| e.to_string())?,
				event = events.recv() => {
					let value = event.map_err(|_| "Research events were lost; recover the saved request")?;

						if (value["method"] == crate::events::UPDATED) != research.load(Ordering::Relaxed) {
							continue;
						}

					write.send(Message::Text(value.to_string().into())).await.map_err(|e| e.to_string())?;
				},
			}
		}

		#[allow(unreachable_code)]
		Ok(())
	};

	tokio::select! {
		result = read_loop => result,
		result = write_loop => result,
	}
}

#[cfg(test)]
#[path = "native_host_tests.rs"]
mod tests;
