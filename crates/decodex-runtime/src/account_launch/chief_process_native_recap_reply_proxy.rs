//! Test-only transport fault: execute in the real service, then lose one recap reply.
use decodex_protocol::{
	ChiefActionDto, ClientMessage, CommandOutcome, CommandPayload, QueryPayload, ServerMessage,
};
use futures_util::{SinkExt, StreamExt};
use std::{
	path::{Path, PathBuf},
	sync::{
		Arc, Mutex,
		atomic::{AtomicUsize, Ordering},
	},
};
use tokio::net::UnixStream;
use tokio_tungstenite::tungstenite::Message;

#[derive(Default)]
struct Evidence {
	commands: Mutex<Vec<String>>,
	dropped: AtomicUsize,
	readbacks: AtomicUsize,
}

pub(super) struct Proxy {
	pub(super) root: PathBuf,
	task: tokio::task::JoinHandle<()>,
	evidence: Arc<Evidence>,
}

impl Proxy {
	pub(super) async fn start(home: &Path) -> Self {
		use std::os::unix::fs::PermissionsExt;
		let root = home.join("recap-proxy");
		std::fs::create_dir(&root).unwrap();
		std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
		let server = root.join("server");
		std::fs::create_dir(&server).unwrap();
		std::fs::set_permissions(&server, std::fs::Permissions::from_mode(0o700)).unwrap();
		std::fs::copy(home.join("product/config.toml"), root.join("config.toml")).unwrap();
		let socket = server.join("decodex.sock");
		let listener = tokio::net::UnixListener::bind(&socket).unwrap();
		std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
		let upstream = home.join("product/server/decodex.sock");
		let evidence = Arc::new(Evidence::default());
		let observed = evidence.clone();
		let task = tokio::spawn(async move {
			let mut connections = tokio::task::JoinSet::new();
			loop {
				tokio::select! {
					accepted = listener.accept() => {
						let (stream, _) = accepted.unwrap();
						let upstream = upstream.clone();
						let observed = observed.clone();
						connections.spawn(async move { forward(stream, upstream, observed).await; });
					},
					Some(result) = connections.join_next() => { result.unwrap(); },
				}
			}
		});
		Self { root, task, evidence }
	}

	pub(super) fn verify(&self, request_id: &serde_json::Value, home: &Path) {
		let commands = self.evidence.commands.lock().unwrap();
		assert_eq!(commands.len(), 1, "desktop must not replay generation");
		assert_eq!(request_id.as_str(), Some(commands[0].as_str()));
		assert_eq!(self.evidence.dropped.load(Ordering::Acquire), 1);
		let readbacks = self.evidence.readbacks.load(Ordering::Acquire);
		assert!(readbacks > 0, "lost reply must use real service readback");
		std::fs::write(home.join("recap-lost-reply.json"), serde_json::to_vec_pretty(&serde_json::json!({"generation_commands":*commands,"dropped_successful_results":1,"subsequent_status_reads":readbacks})).unwrap()).unwrap();
	}
}

impl Drop for Proxy {
	fn drop(&mut self) {
		self.task.abort();
	}
}

async fn forward(stream: UnixStream, path: PathBuf, evidence: Arc<Evidence>) {
	let Ok(mut downstream) = tokio_tungstenite::accept_async(stream).await else { return };
	let stream = UnixStream::connect(path).await.unwrap();
	let (mut upstream, _) =
		tokio_tungstenite::client_async("ws://localhost/v1/ws", stream).await.unwrap();
	let mut selected = None;
	loop {
		tokio::select! {
			message = downstream.next() => {
				let Some(Ok(message)) = message else { return };
				if let Message::Text(text) = &message {
					match serde_json::from_str::<ClientMessage>(text).unwrap() {
						ClientMessage::Command(command) if matches!(&command.payload, CommandPayload::Chief { action } if matches!(**action, ChiefActionDto::GenerateRecap { .. })) => {
							evidence.commands.lock().unwrap().push(command.idempotency_key.as_str().into());
							selected = Some(command.client_command_id);
						},
						ClientMessage::Query(query) if matches!(query.payload, QueryPayload::GetChiefRecap { .. }) && evidence.dropped.load(Ordering::Acquire) > 0 => { evidence.readbacks.fetch_add(1, Ordering::AcqRel); },
						_ => {},
					}
				}
				if upstream.send(message).await.is_err() { return; }
			},
			message = upstream.next() => {
				let Some(Ok(message)) = message else { return };
				if let Message::Text(text) = &message {
					match serde_json::from_str::<ServerMessage>(text).unwrap() {
						ServerMessage::CommandReceipt(receipt) if selected.as_ref() == Some(&receipt.client_command_id) => { continue; },
						ServerMessage::CommandResult(result) if selected.as_ref() == Some(&result.client_command_id) && evidence.dropped.load(Ordering::Acquire) == 0 => {
							assert_eq!(result.outcome, CommandOutcome::Succeeded, "fault must follow real execution");
							evidence.dropped.fetch_add(1, Ordering::AcqRel);
							let _ = downstream.close(None).await;
							return;
						},
						_ => {},
					}
				}
				if downstream.send(message).await.is_err() { return; }
			},
		}
	}
}
