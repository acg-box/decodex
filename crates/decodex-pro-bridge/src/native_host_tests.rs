use super::*;
use tokio::io::{duplex, split};

#[tokio::test]
async fn local_clients_share_one_native_connection_and_recover_after_host_exit() {
	let root = tempfile::tempdir().unwrap();
	let path = socket_path(root.path());
	for _ in 0..2 {
		let (native, extension) = duplex(8192);
		let (read, write) = split(native);
		let directory = root.path().to_owned();
		let host = tokio::spawn(async move { serve_io(&directory, read, write).await });
		let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
		while !path.exists() {
			assert!(tokio::time::Instant::now() < deadline);
			tokio::task::yield_now().await;
		}
		let (mut first, _) = tokio_tungstenite::client_async(
			"ws://localhost/research",
			UnixStream::connect(&path).await.unwrap(),
		)
		.await
		.unwrap();
		let (mut second, _) = tokio_tungstenite::client_async(
			"ws://localhost/research",
			UnixStream::connect(&path).await.unwrap(),
		)
		.await
		.unwrap();
		first
			.send(Message::Text(json!({"id":1,"method":"first"}).to_string().into()))
			.await
			.unwrap();
		second
			.send(Message::Text(json!({"id":1,"method":"second"}).to_string().into()))
			.await
			.unwrap();
		let (mut read, mut write) = split(extension);
		let one = native_wire::read_packet(&mut read).await.unwrap();
		let two = native_wire::read_packet(&mut read).await.unwrap();
		assert_ne!(one["id"], two["id"]);
		for packet in [two, one] {
			native_wire::write_packet(
				&mut write,
				&json!({"id":packet["id"],"result":{"owner":packet["method"],"text":"完整🦀".repeat(30_000)}}),
			)
			.await
			.unwrap();
		}
		for (mut socket, owner) in [(first, "first"), (second, "second")] {
			let Message::Text(reply) = socket.next().await.unwrap().unwrap() else {
				panic!("missing reply")
			};
			let value: Value = serde_json::from_str(&reply).unwrap();
			assert_eq!(value["id"], 1);
			assert_eq!(value["result"]["owner"], owner);
			assert_eq!(value["result"]["text"].as_str().unwrap().chars().count(), 90_000);
			socket.close(None).await.unwrap();
		}
		drop(write);
		drop(read);
		assert!(
			tokio::time::timeout(Duration::from_secs(2), host).await.unwrap().unwrap().is_err()
		);
		assert!(!path.exists(), "host shutdown must remove only its own socket");
	}
}
