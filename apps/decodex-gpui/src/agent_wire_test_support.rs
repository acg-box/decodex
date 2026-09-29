//! Shared local socket setup for UI wire tests; scenarios and assertions stay with each feature.
use super::ClientProfile;
use std::{
	future::Future,
	os::unix::fs::{MetadataExt, PermissionsExt},
};

pub(super) const SERVER: &str = "018f0f9e-7b6e-4a31-8f4c-1d2e3f405162";

pub(super) fn fixture<T: Send + 'static, F, Fut>(
	serve: F,
) -> (tempfile::TempDir, ClientProfile, std::thread::JoinHandle<T>)
where
	F: FnOnce(tokio::net::UnixListener) -> Fut + Send + 'static,
	Fut: Future<Output = T>,
{
	let root = tempfile::tempdir_in("/tmp").unwrap();
	let path = root.path().canonicalize().unwrap();
	let server = path.join("server");
	std::fs::create_dir(&server).unwrap();
	std::fs::set_permissions(&server, std::fs::Permissions::from_mode(0o700)).unwrap();
	let uid = std::fs::metadata(&path).unwrap().uid();
	let config = path.join("config.toml");
	std::fs::write(&config, format!("version = 1\nactive_profile = \"local\"\ncache = {{}}\n[profiles.local]\nkind = \"local\"\npolicy = \"same_uid\"\nservice_owner_uid = {uid}\nexpected_server_identity = \"{SERVER}\"\n")).unwrap();
	std::fs::set_permissions(config, std::fs::Permissions::from_mode(0o600)).unwrap();
	let socket_path = server.join("decodex.sock");
	let listener = std::os::unix::net::UnixListener::bind(&socket_path).unwrap();
	std::fs::set_permissions(socket_path, std::fs::Permissions::from_mode(0o600)).unwrap();
	listener.set_nonblocking(true).unwrap();
	let profile = ClientProfile::load(&path, None).unwrap();
	let thread = std::thread::spawn(move || {
		let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
		runtime.block_on(async {
			let listener = tokio::net::UnixListener::from_std(listener).unwrap();
			tokio::time::timeout(std::time::Duration::from_secs(5), serve(listener)).await.unwrap()
		})
	});
	(root, profile, thread)
}

/// Complete the common welcome exchange. Feature tests inspect the following request.
pub(super) async fn accept(
	listener: &tokio::net::UnixListener,
) -> tokio_tungstenite::WebSocketStream<tokio::net::UnixStream> {
	use decodex_protocol::{
		CURRENT_VERSION, Cursor, ReconnectMode, ServerId, ServerMessage, ServerWelcome,
		SnapshotEnvelope,
	};
	use futures_util::{SinkExt, StreamExt};
	use tokio_tungstenite::tungstenite::Message;
	let mut socket =
		tokio_tungstenite::accept_async(listener.accept().await.unwrap().0).await.unwrap();
	let _hello = socket.next().await.unwrap().unwrap();
	for message in [
		ServerMessage::Welcome(ServerWelcome {
			version: CURRENT_VERSION,
			server_id: ServerId::new(SERVER).unwrap(),
			instance_id: None,
			cursor: Cursor(0),
			reconnect: ReconnectMode::Snapshot,
		}),
		ServerMessage::Snapshot(SnapshotEnvelope {
			version: CURRENT_VERSION,
			server_id: ServerId::new(SERVER).unwrap(),
			cursor: Cursor(0),
			items: vec![],
		}),
	] {
		socket.send(Message::Text(serde_json::to_string(&message).unwrap().into())).await.unwrap();
	}
	socket
}
