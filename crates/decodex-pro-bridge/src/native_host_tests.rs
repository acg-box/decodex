use std::time::Duration;

use futures_util::{SinkExt as _, StreamExt as _};
use serde_json::Value;
use tokio::{
	io::{self},
	net::UnixStream,
	task,
	time::{self, Instant},
};
use tokio_tungstenite::tungstenite::Message;

use crate::{native_host, native_wire};

#[tokio::test]
async fn large_browser_events_do_not_block_an_incoming_long_prompt() {
	let root = tempfile::tempdir().unwrap();
	let (native, extension) = io::duplex(8_192);
	let (read, write) = io::split(native);
	let directory = root.path().to_owned();
	let host = tokio::spawn(async move {
		native_host::serve_io(&directory, read, write, "fixture-browser".into()).await
	});

	while !native_host::socket_path(root.path()).exists() {
		task::yield_now().await;
	}

	let (mut client, _) = tokio_tungstenite::client_async(
		"ws://localhost/research",
		UnixStream::connect(native_host::socket_path(root.path())).await.unwrap(),
	)
	.await
	.unwrap();
	let (mut read, mut write) = io::split(extension);
	let text = "context ".repeat(300_000);

	native_wire::write_packet(
		&mut write,
		&serde_json::json!({"sessionId":"owned","method":"Network.requestWillBeSent","params":{"text":text}}),
	)
	.await
	.unwrap();
	// The browser event exceeds the Unix socket buffer. The client deliberately
	// sends its next large command before reading that event.
	time::sleep(Duration::from_millis(20)).await;
	time::timeout(
		Duration::from_secs(2),
		client.send(Message::Text(
			serde_json::json!({"id":1,"method":"Input.insertText","sessionId":"owned","params":{"text":text}})
				.to_string()
				.into(),
		)),
	)
	.await
	.expect("A pending browser event must not deadlock a large outgoing prompt")
	.unwrap();

	let request = native_wire::read_packet(&mut read).await.unwrap();

	assert_eq!(request["params"]["text"], text);

	let Message::Text(event) = client.next().await.unwrap().unwrap() else {
		panic!("Missing event")
	};

	assert_eq!(serde_json::from_str::<Value>(&event).unwrap()["params"]["text"], text);

	native_wire::write_packet(&mut write, &serde_json::json!({"id":request["id"],"result":true}))
		.await
		.unwrap();

	let Message::Text(reply) = client.next().await.unwrap().unwrap() else {
		panic!("Missing reply")
	};

	assert_eq!(serde_json::from_str::<Value>(&reply).unwrap()["result"], true);

	drop(write);
	drop(read);

	assert!(host.await.unwrap().is_err());
}

#[tokio::test]
async fn local_clients_share_one_native_connection_and_recover_after_host_exit() {
	let root = tempfile::tempdir().unwrap();
	let path = native_host::socket_path(root.path());

	for _ in 0..2 {
		let (native, extension) = io::duplex(8_192);
		let (read, write) = io::split(native);
		let directory = root.path().to_owned();
		let host = tokio::spawn(async move {
			native_host::serve_io(&directory, read, write, "fixture-browser-session".into()).await
		});
		let deadline = Instant::now() + Duration::from_secs(3);

		while !path.exists() {
			assert!(Instant::now() < deadline);

			task::yield_now().await;
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
		let (mut read, mut write) = io::split(extension);

		native_wire::write_packet(&mut write, &serde_json::json!({"kind":"hello"})).await.unwrap();

		let session = native_wire::read_packet(&mut read).await.unwrap();

		assert_eq!(
			session,
			serde_json::json!({"kind":"session","browserSession":"fixture-browser-session"})
		);

		first
			.send(Message::Text(serde_json::json!({"id":1,"method":"first"}).to_string().into()))
			.await
			.unwrap();
		second
			.send(Message::Text(serde_json::json!({"id":1,"method":"second"}).to_string().into()))
			.await
			.unwrap();

		let one = native_wire::read_packet(&mut read).await.unwrap();
		let two = native_wire::read_packet(&mut read).await.unwrap();

		assert_ne!(one["id"], two["id"]);

		for packet in [two, one] {
			native_wire::write_packet(
				&mut write,
				&serde_json::json!({"id":packet["id"],"result":{"owner":packet["method"],"text":"完整🦀".repeat(30_000)}}),
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

		assert!(time::timeout(Duration::from_secs(2), host).await.unwrap().unwrap().is_err());
		assert!(!path.exists(), "host shutdown must remove only its own socket");
	}
}
