use crate::agent::json_rpc::{JsonRpcMessage, WireMessage};

#[test]
fn parses_notification_messages() {
	let message = WireMessage::parse(
		r#"{"method":"thread/status/changed","params":{"threadId":"thread-1"}}"#.to_owned(),
	)
	.expect("notification should parse");

	match message.message {
		JsonRpcMessage::Notification(notification) => {
			assert_eq!(notification.method, "thread/status/changed");
			assert_eq!(notification.params["threadId"], serde_json::json!("thread-1"));
		},
		other => panic!("unexpected message: {other:?}"),
	}
}

#[test]
fn parses_response_messages() {
	let message =
		WireMessage::parse(r#"{"id":1,"result":{"userAgent":"decodex-test"}}"#.to_owned())
			.expect("response should parse");

	match message.message {
		JsonRpcMessage::Response(response) => {
			assert_eq!(response.id, serde_json::json!(1));
			assert_eq!(response.result["userAgent"], serde_json::json!("decodex-test"));
		},
		other => panic!("unexpected message: {other:?}"),
	}
}
