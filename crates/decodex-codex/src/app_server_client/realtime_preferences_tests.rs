//! The account receives one explicit write with the advertised ID.
use serde_json::Value;
use tokio::io::{self, AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};

use crate::app_server_client::AppServerClient;

#[tokio::test]
async fn lost_voice_write_reply_does_not_repeat_the_native_edit() {
	let (local, remote) = io::duplex(4096);
	let (read, write) = io::split(local);
	let (client, _events) = AppServerClient::from_io(read, write);
	let server = tokio::spawn(async move {
		let (read, mut write) = io::split(remote);
		let mut lines = BufReader::new(read).lines();
		let request: Value =
			serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
		assert_eq!(request["method"], "account/voiceSettings/read");
		assert!(request["params"].is_null());
		let response = serde_json::json!({"id":request["id"],"result":{"selected":"old","voices":[{"voice":"new","name":"New voice"}]}});
		write.write_all(format!("{response}\n").as_bytes()).await.unwrap();
		let request: Value =
			serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
		assert_eq!(request["method"], "account/voiceSettings/write");
		assert_eq!(request["params"], serde_json::json!({"voice":"new"}));
	});

	let observed = client.realtime_voice_settings().await.unwrap();
	assert!(client.write_realtime_voice(&observed, "New voice").await.is_err());
	assert!(client.write_realtime_voice(&observed, "new").await.is_err());
	server.await.unwrap();
}
